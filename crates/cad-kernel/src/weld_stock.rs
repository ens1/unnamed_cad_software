//! Exact sketch-derived member solids, joint end planes and accessory stock.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{CadError, Dimension, Document, NodeRef, Plane, Result, SketchCurve, weldment::*};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap};
const EPS: f64 = 1e-7;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Weldment: {}", message.to_string()))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Weldment: {message}"))
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
    Ok(Plane {
        origin: origin.into(),
        u: u.into(),
        v: normal.cross(u).into(),
    })
}
fn normal(plane: &Plane) -> Vector3 {
    Vector3::from(plane.u)
        .cross(Vector3::from(plane.v))
        .normalize()
}
fn project_face(face: &Face, axis: Vector3, target: &Plane) -> Result<Face> {
    let n = normal(target);
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
#[derive(Clone)]
enum Carrier {
    Line,
    Circle {
        center: Point3,
        normal: Vector3,
        radius: f64,
        angle: f64,
        closed: bool,
    },
}
#[derive(Clone)]
struct Segment {
    id: String,
    a: Point3,
    b: Point3,
    start_axis: Vector3,
    end_axis: Vector3,
    carrier: Carrier,
    spatial_sources: Option<Vec<cad_core::TopologyRef>>,
}
impl Segment {
    fn closed(&self) -> bool {
        matches!(self.carrier, Carrier::Circle { closed: true, .. })
    }
    fn direction_away(&self, position: Point3) -> Option<Vector3> {
        if self.closed() {
            None
        } else if self.a.distance(position) <= EPS {
            Some(self.start_axis)
        } else if self.b.distance(position) <= EPS {
            Some(-self.end_axis)
        } else {
            None
        }
    }
    fn contains_joint(&self, position: Point3) -> bool {
        if self.direction_away(position).is_some() {
            return true;
        }
        if !matches!(self.carrier, Carrier::Line) {
            return false;
        }
        let along = (position - self.a).dot(self.start_axis);
        along > EPS
            && along < (self.b - self.a).magnitude() - EPS
            && (position - self.a - self.start_axis * along).magnitude() <= EPS
    }
}
struct BuiltMember {
    reference: MemberRef,
    start: Face,
    finish: Face,
    start_axis: Vector3,
    end_axis: Vector3,
    solid: Solid,
    metadata: EvaluatedMember,
    start_terminal: bool,
    end_terminal: bool,
    origins: Vec<Vec<crate::section_history::Member>>,
}
struct Built {
    solids: Vec<Solid>,
    metadata: Vec<EvaluatedMember>,
    accessories: Vec<EvaluatedAccessory>,
}
struct Placement<'a> {
    section: &'a Face,
    origin: Point3,
    u: Vector3,
    v: Vector3,
    n: Vector3,
    path_n: Vector3,
    roll: f64,
    profile_offset: [f64; 2],
    spatial: bool,
    clock_u: Vector3,
    rotationally_symmetric: bool,
    rectangle_symmetry: Option<bool>,
}
impl Placement<'_> {
    fn axes(&self, axis: Vector3) -> (Vector3, Vector3) {
        let rotation = Matrix3::from_axis_angle(axis, Rad(self.roll));
        let transverse = if self.spatial && self.path_n.cross(axis).magnitude() <= 1e-10 {
            self.clock_u
        } else {
            self.path_n
        };
        let u = rotation * transverse.cross(axis).normalize();
        let v = axis.cross(u);
        (u, v)
    }
    fn miter_clocking(&self, first: &Segment, second: &Segment, normal: Vector3) -> bool {
        if self.rotationally_symmetric {
            return true;
        }
        let Some(swappable) = self.rectangle_symmetry else {
            return false;
        };
        let (u, v) = self.axes(first.start_axis);
        let (other_u, other_v) = self.axes(second.start_axis);
        let reflect = |vector: Vector3| vector - 2. * normal * normal.dot(vector);
        let parallel = |a: Vector3, b: Vector3| a.cross(b).magnitude() <= 1e-10;
        (parallel(reflect(u), other_u) && parallel(reflect(v), other_v))
            || (swappable && parallel(reflect(u), other_v) && parallel(reflect(v), other_u))
    }
    fn at(&self, position: Point3, axis: Vector3) -> Face {
        let (u, v) = self.axes(axis);
        let map = |vector: Vector3| {
            u * vector.dot(self.u) + v * vector.dot(self.v) + axis * vector.dot(self.n)
        };
        let translation = position.to_vec() - map(self.origin.to_vec());
        let translation = if self.profile_offset == [0.; 2] {
            translation
        } else {
            translation + u * self.profile_offset[0] + v * self.profile_offset[1]
        };
        builder::transformed(
            self.section,
            Matrix4::from_cols(
                map(Vector3::unit_x()).extend(0.),
                map(Vector3::unit_y()).extend(0.),
                map(Vector3::unit_z()).extend(0.),
                translation.extend(1.),
            ),
        )
    }
}
/// Full authored rectangles centered exactly on the layout path are invariant
/// under section-axis sign changes. Equal sides additionally admit X/Y swaps.
/// This is source/carrier admission, not a sampled silhouette comparison.
fn rectangle_symmetry(
    source: &cad_core::Sketch,
    face: &Face,
    members: &std::collections::HashMap<EdgeId, (String, usize)>,
    center: [f64; 2],
) -> Option<bool> {
    if center != [0.; 2] {
        return None;
    }
    let mut swappable = true;
    for wire in face.boundaries() {
        if wire.len() != 4 {
            return None;
        }
        let mut points = vec![];
        for edge in &wire {
            let (member, _) = members.get(&edge.id())?;
            let SketchCurve::Line { start, end } =
                cad_core::sketch_edit::effective_curve(source, member).ok()?
            else {
                return None;
            };
            let a = *source.points.get(&start)?;
            let b = *source.points.get(&end)?;
            if (a[0] == b[0]) == (a[1] == b[1]) {
                return None;
            }
            for point in [a, b] {
                if !points.contains(&point) {
                    points.push(point);
                }
            }
        }
        if points.len() != 4 {
            return None;
        }
        let radii = [
            points.iter().map(|p| p[0].abs()).fold(0., f64::max),
            points.iter().map(|p| p[1].abs()).fold(0., f64::max),
        ];
        if radii.iter().any(|r| !r.is_finite() || *r <= EPS)
            || points.iter().any(|p| {
                (p[0] != radii[0] && p[0] != -radii[0]) || (p[1] != radii[1] && p[1] != -radii[1])
            })
        {
            return None;
        }
        swappable &= radii[0] == radii[1];
    }
    Some(swappable)
}
/// Butt priority is saved member selection order (stable source ID order for All).
/// The outer polygon must be convex and the locating point inside it: these
/// support planes are exact stock surfaces, rather than tessellated collisions.
fn butt_plane(
    primary: &Segment,
    position: Point3,
    away: Vector3,
    placement: &Placement<'_>,
    gap: f64,
) -> Result<Plane> {
    if !matches!(primary.carrier, Carrier::Line) {
        return Err(unsupported(
            "Prioritized butt trimming needs a straight primary member; circular stock supports tangent square joints",
        ));
    }
    let direction = primary.direction_away(position);
    if direction
        .is_some_and(|direction| direction.dot(away) > -1. + 1e-8 && direction.dot(away) < -1e-8)
    {
        return Err(unsupported(
            "This butt branch crosses the primary terminal cap; use an explicit end trim plane",
        ));
    }
    let face = placement.at(position, primary.start_axis);
    let wire = face
        .boundaries()
        .first()
        .cloned()
        .ok_or_else(|| invalid("Primary stock has no outer section"))?;
    if wire.len() < 3
        || wire
            .iter()
            .any(|edge| !matches!(edge.curve(), Curve::Line(_)))
    {
        return Err(unsupported(
            "Prioritized butt trim needs a convex straight-sided outer section; circular/spline wall coping requires a native body trim",
        ));
    }
    let points = wire
        .iter()
        .map(|edge| edge.front().point())
        .collect::<Vec<_>>();
    let centroid = points.iter().map(|point| point.to_vec()).sum::<Vector3>() / points.len() as f64;
    let mut candidates = vec![];
    for edge in &wire {
        let anchor = edge.front().point();
        let mut n = primary
            .start_axis
            .cross(edge.back().point() - anchor)
            .normalize();
        if (centroid - anchor.to_vec()).dot(n) > 0. {
            n = -n;
        }
        if points.iter().any(|point| (*point - anchor).dot(n) > EPS) {
            return Err(unsupported(
                "Butt trim outer section is concave; author an explicit end trim plane or split its stock group",
            ));
        }
        if (position - anchor).dot(n) > EPS {
            return Err(invalid(
                "Butt-trim locating point must be inside the primary outer contour",
            ));
        }
        let dot = away.dot(n);
        if dot > 1e-9 {
            let distance = (anchor - position).dot(n) / dot;
            if distance >= -EPS {
                candidates.push((distance, anchor, n));
            }
        }
    }
    let (_, anchor, n) = candidates.into_iter().min_by(|a,b|a.0.total_cmp(&b.0)).ok_or_else(|| unsupported("A collinear butt branch has no transverse primary wall; use Miter or an explicit trim plane"))?;
    plane(anchor + n * gap, n)
}
fn joint(
    segment: &Segment,
    position: Point3,
    away: Vector3,
    segments: &[Segment],
    definition: &Group,
    placement: &Placement<'_>,
) -> Result<(Plane, bool)> {
    let others = segments
        .iter()
        .filter(|other| other.id != segment.id && other.contains_joint(position))
        .collect::<Vec<_>>();
    let mut target = plane(position, away)?;
    if others.is_empty() {
        return Ok((target, true));
    }
    match definition.corner {
        Corner::Miter => {
            if others.len() != 1 {
                return Err(unsupported(
                    "Automatic miters need two incident members; use prioritized Butt Trim for branches or explicit trim planes",
                ));
            }
            let direction = others[0].direction_away(position).ok_or_else(||unsupported("A member ending on another member's interior is a branch junction; choose Trim to Primary Member or an explicit trim plane"))?;
            let n = away - direction;
            if n.magnitude() <= EPS {
                return Err(invalid(
                    "Coincident outgoing axes cannot define a positive miter junction",
                ));
            }
            if placement.spatial
                && !placement.miter_clocking(segment, others[0], n.normalize())
                && (away.dot(placement.path_n).abs() > 1e-10
                    || direction.dot(placement.path_n).abs() > 1e-10)
            {
                return Err(unsupported(
                    "A spatial miter on this section needs compatible joint clocking; use Independent Butt or explicit trim planes, or align the sketch workplane with the two incident axes. Centered circular and proven reflected rectangular stock admit compatible frames",
                ));
            }
            if (!matches!(segment.carrier, Carrier::Line)
                || !matches!(others[0].carrier, Carrier::Line))
                && away.dot(direction) > -1. + 1e-8
            {
                return Err(unsupported(
                    "Circular stock can join with matching tangents; angled circular miters need an exact non-normal end trim",
                ));
            }
            target = plane(position, n)?;
            let n = normal(&target)
                * if normal(&target).dot(away) < 0. {
                    -1.
                } else {
                    1.
                };
            target.origin = (position + n * definition.joint_gap_mm * 0.5).into();
        }
        Corner::Butt => {
            target.origin = (position + away * definition.joint_gap_mm * 0.5).into();
        }
        Corner::ButtTrim => {
            let primary = segments
                .iter()
                .find(|other| other.contains_joint(position))
                .unwrap();
            if primary.id != segment.id {
                if primary
                    .direction_away(position)
                    .is_some_and(|direction| direction.dot(away) < -1. + 1e-8)
                {
                    target.origin = (position + away * definition.joint_gap_mm).into();
                } else {
                    target =
                        butt_plane(primary, position, away, placement, definition.joint_gap_mm)?;
                }
            }
        }
    }
    Ok((target, false))
}
fn arc_trim(segment: &Segment, target: &Plane, previous: f64) -> Result<f64> {
    let Carrier::Circle {
        center,
        normal: axis,
        radius,
        angle,
        ..
    } = segment.carrier
    else {
        return Err(invalid("Expected circular member"));
    };
    let radial = (segment.a - center) / radius;
    let tangent = axis.cross(radial) * angle.signum();
    let n = normal(target);
    let a = n.dot(radial) * radius;
    let b = n.dot(tangent) * radius;
    let c = n.dot(center - Point3::from(target.origin));
    let extent = a.hypot(b);
    if extent <= EPS || c.abs() > extent + EPS {
        return Err(invalid("Trim plane does not cross the circular centerline"));
    }
    let phase = b.atan2(a);
    let delta = (-c / extent).clamp(-1., 1.).acos();
    let mut candidates = [phase - delta, phase + delta]
        .into_iter()
        .map(|theta| theta.rem_euclid(std::f64::consts::TAU))
        .collect::<Vec<_>>();
    // Endpoint planes may also meet at a seam represented as 2π.
    if candidates.iter().any(|theta| theta.abs() <= 1e-9) {
        candidates.push(std::f64::consts::TAU);
    }
    let theta = candidates
        .into_iter()
        .min_by(|a, b| (a - previous).abs().total_cmp(&(b - previous).abs()))
        .unwrap();
    let rotation = Matrix3::from_axis_angle(axis, Rad(theta * angle.signum()));
    let direction = rotation * segment.start_axis;
    if n.cross(direction).magnitude() > 1e-8 {
        return Err(unsupported(
            "Circular member end trimming requires a plane normal to the local tangent; angled circle/plane intersections require native curved stock coping",
        ));
    }
    Ok(theta)
}
fn member_history(
    generated: &mut TopologyHistory,
    definition: &Group,
    segment: &Segment,
    solid: &Solid,
    origins: &[crate::section_history::Member],
    kind: MemberPathKind,
    sweep: Option<f64>,
) -> Result<()> {
    let path = if let Some(sources) = &segment.spatial_sources {
        crate::section_history::Member {
            sources: sources.clone(),
            purpose: serde_json::json!({"spatial_member":segment.id}),
        }
    } else {
        crate::section_history::member(
            &definition.path,
            &crate::extrusion::ThinMember {
                sources: vec![segment.id.clone()],
                side: 0,
                piece: 0,
                cap: false,
            },
        )?
    };
    let sources = origins
        .iter()
        .flat_map(|origin| origin.sources.clone())
        .chain(path.sources.clone())
        .collect::<Vec<_>>();
    let mut history =
        crate::section_history::History::new(&definition.source, "weldment_member", generated);
    let mut tag = |face: &Face, origin: &crate::section_history::Member, station: usize| {
        let sources = origin
            .sources
            .iter()
            .cloned()
            .chain(path.sources.clone())
            .collect::<Vec<_>>();
        history.face(
            face,
            &sources,
            serde_json::json!({"member":segment.id,"wall":origin.purpose,"station":station}),
        )
    };
    let faces = solid.face_iter().collect::<Vec<_>>();
    let divisions = if kind == MemberPathKind::Straight {
        1
    } else {
        (sweep.unwrap().abs() / std::f64::consts::FRAC_PI_2)
            .ceil()
            .max(2.) as usize
    };
    if segment.closed() {
        if faces.len() != divisions * origins.len() {
            return Err(unsupported(
                "Circular constructor changed member section topology",
            ));
        }
        // Closed revolution keeps one shell for each source material contour.
        let mut offset = 0;
        for shell in solid.boundaries() {
            if shell.len() % divisions != 0 {
                return Err(invalid(
                    "Closed member contour station topology is inconsistent",
                ));
            }
            let count = shell.len() / divisions;
            for (index, face) in shell.iter().enumerate() {
                tag(face, &origins[offset + index % count], index / count)?;
            }
            offset += count;
        }
    } else {
        if faces.len() != divisions * origins.len() + 2 {
            return Err(unsupported(
                "Open stock constructor changed member section topology",
            ));
        }
        for (index, face) in faces
            .iter()
            .skip(1)
            .take(divisions * origins.len())
            .enumerate()
        {
            tag(face, &origins[index % origins.len()], index / origins.len())?;
        }
        history.face(
            faces[0],
            &sources,
            serde_json::json!({"member":segment.id,"end":"start"}),
        )?;
        history.face(
            faces.last().unwrap(),
            &sources,
            serde_json::json!({"member":segment.id,"end":"finish"}),
        )?;
    }
    history.finish(solid)
}
fn group(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    definition: &Group,
    trims: &[Trim],
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Vec<BuiltMember>> {
    let (source, section_plane) =
        crate::extrusion::source(adapter, doc, &definition.section, inputs)?;
    let spatial = crate::sketch3d::spatial(inputs, &definition.path);
    let planar = if spatial.is_none() {
        Some(crate::extrusion::source(
            adapter,
            doc,
            &definition.path,
            inputs,
        )?)
    } else {
        None
    };
    let path_plane = spatial
        .map(|spatial| &spatial.solved.workplane)
        .or_else(|| planar.as_ref().map(|(_, plane)| plane))
        .ok_or_else(|| invalid("The structural path source is unavailable"))?;
    let traced = crate::sketches::build_traced(
        doc,
        &definition.section.definition,
        &source,
        &section_plane,
    )?;
    let Shape::Sketch { mut faces, .. } = traced.shape else {
        return Err(invalid("Section does not produce native sketch regions"));
    };
    if faces.len() != 1 {
        return Err(unsupported(
            "Structural members need one section material region, with optional inner contours",
        ));
    }
    let mut face = faces.remove(0);
    if face
        .oriented_surface()
        .normal(0., 0.)
        .dot(Vector3::from(section_plane.u).cross(Vector3::from(section_plane.v)))
        < 0.
    {
        face.invert();
    }
    let section_origins = face
        .boundaries()
        .into_iter()
        .flatten()
        .map(|edge| {
            let (member, piece) = traced
                .members
                .get(&edge.id())
                .ok_or_else(|| invalid("Section member ancestry is missing"))?;
            crate::section_history::member(
                &definition.section,
                &crate::extrusion::ThinMember {
                    sources: vec![member.clone()],
                    side: 0,
                    piece: *piece,
                    cap: false,
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let locating = if let Some(point) = &definition.anchor {
        *source
            .points
            .get(point)
            .ok_or_else(|| invalid("Section locating point was removed"))?
    } else {
        [0.; 2]
    };
    let section_u = Vector3::from(section_plane.u);
    let section_v = Vector3::from(section_plane.v);
    let path_n = Vector3::from(path_plane.u)
        .cross(Vector3::from(path_plane.v))
        .normalize();
    let centered = [
        locating[0] - definition.profile_offset_mm[0],
        locating[1] - definition.profile_offset_mm[1],
    ];
    let circle_centered = |center: &str| source.points.get(center) == Some(&centered);
    let material = source
        .members
        .values()
        .filter(|member| !member.construction)
        .collect::<Vec<_>>();
    let rotationally_symmetric = source.profile_order.is_empty() && source.segments.is_empty()
        && (!material.is_empty() || !source.circles.is_empty())
        && material.iter().all(|member| matches!(&member.curve, SketchCurve::Circle { center, .. } if circle_centered(center)))
        && source.circles.values().all(|circle| circle_centered(&circle.center));
    let rectangle_symmetry = rectangle_symmetry(&source, &face, &traced.members, centered);
    let placement = Placement {
        section: &face,
        origin: Point3::from(section_plane.origin)
            + section_u * locating[0]
            + section_v * locating[1],
        u: section_u,
        v: section_v,
        n: section_u.cross(section_v).normalize(),
        path_n,
        roll: definition.roll_rad,
        profile_offset: definition.profile_offset_mm,
        spatial: spatial.is_some(),
        clock_u: Vector3::from(path_plane.u),
        rotationally_symmetric,
        rectangle_symmetry,
    };
    let library_profile = definition
        .library_profile
        .as_ref()
        .map(ProfileLabel::from_origin)
        .transpose()?;
    let world = |point: [f64; 2]| {
        Point3::from(path_plane.origin)
            + Vector3::from(path_plane.u) * point[0]
            + Vector3::from(path_plane.v) * point[1]
    };
    let point = |id: &str| {
        if let Some(spatial) = spatial {
            return spatial
                .solved
                .points
                .get(id)
                .copied()
                .map(Point3::from)
                .ok_or_else(|| invalid("Spatial path endpoint was removed"));
        }
        planar
            .as_ref()
            .ok_or_else(|| invalid("The planar path source is unavailable"))?
            .0
            .points
            .get(id)
            .copied()
            .map(world)
            .ok_or_else(|| invalid("Path endpoint was removed"))
    };
    let ids = if definition.members.is_empty() {
        if let Some(spatial) = spatial {
            spatial.solved.members.keys().cloned().collect::<Vec<_>>()
        } else {
            planar
                .as_ref()
                .ok_or_else(|| invalid("The planar path source is unavailable"))?
                .0
                .members
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        }
    } else {
        definition.members.clone()
    };
    if ids.is_empty() || ids.len() > 1024 {
        return Err(invalid(
            "A member group needs between 1 and 1024 path members",
        ));
    }
    let mut segments = vec![];
    for id in ids {
        if let Some(spatial) = spatial {
            let member = spatial
                .solved
                .members
                .get(&id)
                .ok_or_else(|| invalid("Selected spatial path member was removed"))?;
            let cad_core::sketch3d::Curve::Line { start, end } = &member.curve else {
                return Err(unsupported(
                    "Spatial structural stock currently accepts straight line members; spatial arcs/splines need an explicitly compatible moving section frame",
                ));
            };
            let a = point(start)?;
            let b = point(end)?;
            if a.distance(b) <= EPS {
                return Err(invalid("Spatial path member has zero length"));
            }
            let axis = (b - a).normalize();
            let spatial_sources =
                crate::sketch3d::member_curves(adapter, inputs, &definition.path, &id)?
                    .into_iter()
                    .map(|(_, source)| source)
                    .collect();
            segments.push(Segment {
                id,
                a,
                b,
                start_axis: axis,
                end_axis: axis,
                carrier: Carrier::Line,
                spatial_sources: Some(spatial_sources),
            });
            continue;
        }
        let path = &planar
            .as_ref()
            .ok_or_else(|| invalid("The planar path source is unavailable"))?
            .0;
        if !path.members.contains_key(&id) {
            return Err(invalid("Selected path member was removed"));
        }
        let segment = match cad_core::sketch_edit::effective_curve(path, &id)? {
            SketchCurve::Line { start, end } => {
                let a = point(&start)?;
                let b = point(&end)?;
                if a.distance(b) <= EPS {
                    return Err(invalid("Path member has zero length"));
                }
                let axis = (b - a).normalize();
                Segment {
                    id,
                    a,
                    b,
                    start_axis: axis,
                    end_axis: axis,
                    carrier: Carrier::Line,
                    spatial_sources: None,
                }
            }
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (center, radius, _, angle) = cad_core::sketch_geometry::arc_frame(
                    path.points[&start],
                    path.points[&through],
                    path.points[&end],
                )?;
                let center = world(center);
                let a = point(&start)?;
                let b = point(&end)?;
                Segment {
                    id,
                    a,
                    b,
                    start_axis: path_n.cross(a - center).normalize() * angle.signum(),
                    end_axis: path_n.cross(b - center).normalize() * angle.signum(),
                    carrier: Carrier::Circle {
                        center,
                        normal: path_n,
                        radius,
                        angle,
                        closed: false,
                    },
                    spatial_sources: None,
                }
            }
            SketchCurve::Circle { center, radius } => {
                let radius = cad_core::expression::value(
                    doc,
                    &definition.path.definition,
                    &radius,
                    Dimension::Length,
                )?;
                if radius <= EPS {
                    return Err(invalid("Circular stock needs positive bend radius"));
                }
                let center = point(&center)?;
                let a = center + Vector3::from(path_plane.u) * radius;
                let axis = path_n.cross(a - center).normalize();
                Segment {
                    id,
                    a,
                    b: a,
                    start_axis: axis,
                    end_axis: axis,
                    carrier: Carrier::Circle {
                        center,
                        normal: path_n,
                        radius,
                        angle: std::f64::consts::TAU,
                        closed: true,
                    },
                    spatial_sources: None,
                }
            }
            _ => {
                return Err(unsupported(
                    "Structural stock supports straight members, circular arcs and full circles; general spline tangent transport needs a rational moving-frame carrier",
                ));
            }
        };
        segments.push(segment);
    }
    let mut built = vec![];
    for segment in &segments {
        let reference = MemberRef {
            group: definition.source.clone(),
            member: segment.id.clone(),
        };
        let relevant = trims
            .iter()
            .filter(|trim| {
                trim.groups.contains(&definition.source)
                    && (trim.members.is_empty() || trim.members.contains(&reference))
            })
            .collect::<Vec<_>>();
        let (mut start, start_terminal) = if segment.closed() {
            (plane(segment.a, segment.start_axis)?, false)
        } else {
            joint(
                segment,
                segment.a,
                segment.start_axis,
                &segments,
                definition,
                &placement,
            )?
        };
        let (mut finish, end_terminal) = if segment.closed() {
            (plane(segment.b, segment.end_axis)?, false)
        } else {
            joint(
                segment,
                segment.b,
                -segment.end_axis,
                &segments,
                definition,
                &placement,
            )?
        };
        let mut start_terminal = start_terminal;
        let mut end_terminal = end_terminal;
        let section = placement.at(segment.a, segment.start_axis);
        let (
            solid,
            first,
            last,
            a,
            b,
            start_axis,
            end_axis,
            length,
            path_kind,
            bend_radius_mm,
            sweep_angle_rad,
        ) = match segment.carrier {
            Carrier::Line => {
                start.origin = (Point3::from(start.origin)
                    + segment.start_axis * definition.start_offset_mm)
                    .into();
                finish.origin = (Point3::from(finish.origin)
                    - segment.end_axis * definition.end_offset_mm)
                    .into();
                for trim in relevant {
                    let mut target = adapter.resolve_surface_plane(doc, &trim.plane, inputs)?;
                    target.origin =
                        (Point3::from(target.origin) + normal(&target) * trim.offset_mm).into();
                    if trim.end == End::Start {
                        start = target;
                        start_terminal = true;
                    } else {
                        finish = target;
                        end_terminal = true;
                    }
                }
                let at = |target: &Plane| -> Result<Point3> {
                    let n = normal(target);
                    let dot = segment.start_axis.dot(n);
                    if dot.abs() < 1e-9 {
                        return Err(invalid("Trim plane is parallel to a selected member axis"));
                    }
                    Ok(segment.a
                        + segment.start_axis * (Point3::from(target.origin) - segment.a).dot(n)
                            / dot)
                };
                let a = at(&start)?;
                let b = at(&finish)?;
                let length = (b - a).dot(segment.start_axis);
                if length <= EPS {
                    return Err(invalid(
                        "Offsets/trim planes collapse or reverse the member centerline",
                    ));
                }
                let first = project_face(&section, segment.start_axis, &start)?;
                let last = project_face(&section, segment.start_axis, &finish)?;
                let solid =
                    crate::extrusion::to_plane(&first, segment.start_axis, 0., &finish, 0.)?;
                (
                    solid,
                    first,
                    last,
                    a,
                    b,
                    segment.start_axis,
                    segment.end_axis,
                    length,
                    MemberPathKind::Straight,
                    None,
                    None,
                )
            }
            Carrier::Circle {
                center,
                normal: rotation_axis,
                radius,
                angle,
                closed,
            } => {
                if closed
                    && (definition.start_offset_mm.abs() > EPS
                        || definition.end_offset_mm.abs() > EPS
                        || !relevant.is_empty())
                {
                    return Err(invalid(
                        "Closed circular stock has no ends to shorten, extend or trim; use an arc for open rolled members",
                    ));
                }
                let mut begin = definition.start_offset_mm / radius;
                let mut end = angle.abs() - definition.end_offset_mm / radius;
                if !closed {
                    // Joint planes are exact only when tangent-normal at the unchanged endpoint.
                    if !start_terminal {
                        if normal(&start).cross(segment.start_axis).magnitude() > 1e-8 {
                            return Err(unsupported("Circular joints need tangent-normal caps"));
                        }
                        begin += definition.joint_gap_mm * 0.5 / radius;
                    }
                    if !end_terminal {
                        if normal(&finish).cross(segment.end_axis).magnitude() > 1e-8 {
                            return Err(unsupported("Circular joints need tangent-normal caps"));
                        }
                        end -= definition.joint_gap_mm * 0.5 / radius;
                    }
                }
                for trim in relevant {
                    let mut target = adapter.resolve_surface_plane(doc, &trim.plane, inputs)?;
                    target.origin =
                        (Point3::from(target.origin) + normal(&target) * trim.offset_mm).into();
                    if trim.end == End::Start {
                        begin = arc_trim(segment, &target, begin)?;
                        start_terminal = true;
                    } else {
                        end = arc_trim(segment, &target, end)?;
                        end_terminal = true;
                    }
                }
                let retained = end - begin;
                if retained <= 1e-9 || (!closed && retained >= std::f64::consts::TAU - 1e-9) {
                    return Err(invalid(
                        "Arc offsets/trim planes collapse or overlap the rolled member",
                    ));
                }
                let start_rotation =
                    Matrix3::from_axis_angle(rotation_axis, Rad(begin * angle.signum()));
                let end_rotation =
                    Matrix3::from_axis_angle(rotation_axis, Rad(end * angle.signum()));
                let a = center + start_rotation * (segment.a - center);
                let b = center + end_rotation * (segment.a - center);
                let start_axis = start_rotation * segment.start_axis;
                let end_axis = end_rotation * segment.start_axis;
                let mut first =
                    builder::rotated(&section, center, rotation_axis, Rad(begin * angle.signum()));
                let last =
                    builder::rotated(&section, center, rotation_axis, Rad(end * angle.signum()));
                if crate::extrusion::face_hull(&first)?
                    .iter()
                    .any(|point| (*point - center).dot((a - center).normalize()) <= EPS)
                {
                    return Err(unsupported(
                        "The curved section reaches its revolution axis; reduce section size or increase bend radius",
                    ));
                }
                if first.oriented_surface().normal(0., 0.).dot(start_axis) < 0. {
                    first.invert();
                }
                let solid: Solid = builder::revolve(
                    &first,
                    center,
                    rotation_axis * angle.signum(),
                    if closed {
                        builder::SweepAngle::Closed
                    } else {
                        builder::SweepAngle::Partial(Rad(retained))
                    },
                    (retained / std::f64::consts::FRAC_PI_2).ceil().max(2.) as usize,
                );
                start = plane(a, start_axis)?;
                finish = plane(b, end_axis)?;
                (
                    solid,
                    first,
                    last,
                    a,
                    b,
                    start_axis,
                    end_axis,
                    radius * retained,
                    if closed {
                        MemberPathKind::Circular
                    } else {
                        MemberPathKind::CircularArc
                    },
                    Some(radius),
                    Some(retained * angle.signum()),
                )
            }
        };
        member_history(
            generated,
            definition,
            segment,
            &solid,
            &section_origins,
            path_kind,
            sweep_angle_rad,
        )?;
        let metadata = EvaluatedMember {
            reference: reference.clone(),
            section: definition.section.clone(),
            profile_offset_mm: definition.profile_offset_mm,
            library_profile: library_profile.clone(),
            start: a.into(),
            end: b.into(),
            centerline_length_mm: length,
            start_cut_angle_rad: normal(&start).dot(start_axis).abs().clamp(0., 1.).acos(),
            end_cut_angle_rad: normal(&finish).dot(end_axis).abs().clamp(0., 1.).acos(),
            path_kind,
            start_tangent: start_axis.into(),
            end_tangent: end_axis.into(),
            bend_radius_mm,
            sweep_angle_rad,
            closed: segment.closed(),
        };
        built.push(BuiltMember {
            reference,
            start: first,
            finish: last,
            start_axis,
            end_axis,
            solid,
            metadata,
            start_terminal,
            end_terminal,
            origins: face
                .boundaries()
                .iter()
                .scan(0, |offset, wire| {
                    let origins = section_origins[*offset..*offset + wire.len()].to_vec();
                    *offset += wire.len();
                    Some(origins)
                })
                .collect(),
        });
    }
    Ok(built)
}
/// Contour sizing is rebuilt from exact lines and native circular edges in its
/// own plane. The disposable sketch is never saved or made authoritative.
fn sized_cap(
    doc: &Document,
    owner: &str,
    face: &Face,
    wire: Wire,
    offset: f64,
    origins: &[crate::section_history::Member],
) -> Result<(Face, Vec<crate::section_history::Member>)> {
    if offset.abs() <= 1e-12 {
        return Ok((
            builder::try_attach_plane(vec![wire]).map_err(invalid)?,
            origins.to_vec(),
        ));
    }
    let frame = crate::adapter::cad_plane(face);
    let origin = Point3::from(frame.origin);
    let u = Vector3::from(frame.u);
    let v = Vector3::from(frame.v);
    let local = |point: Point3| {
        let d = point - origin;
        [d.dot(u), d.dot(v)]
    };
    let mut sketch: cad_core::Sketch = serde_json::from_value(
        serde_json::json!({"id":"weldment-cap-contour","name":"Derived cap contour","points":{},"segments":[],"profile_order":[]}),
    )?;
    let mut vertices = HashMap::new();
    for (index, edge) in wire.iter().enumerate() {
        for vertex in [edge.front(), edge.back()] {
            vertices
                .entry(vertex.id())
                .or_insert_with(|| format!("cap-vertex-{index}-{}", sketch.points.len()));
            let key = vertices[&vertex.id()].clone();
            sketch.points.insert(key, local(vertex.point()));
        }
    }
    let mut selected = vec![];
    for (index, edge) in wire.iter().enumerate() {
        let curve = edge.oriented_curve();
        let start = vertices[&edge.front().id()].clone();
        let end = vertices[&edge.back().id()].clone();
        let member = format!("cap-edge-{index}");
        let geometry = if matches!(curve, Curve::Line(_)) {
            SketchCurve::Line { start, end }
        } else {
            crate::axis_geometry::circle_geometry(&curve).map_err(|_|unsupported("Cap clearance supports exact line/circular-arc contours; spline or obliquely cut ellipse offsets require exact parallel carriers"))?;
            let (a, b) = curve.range_tuple();
            let through = format!("cap-through-{index}");
            sketch
                .points
                .insert(through.clone(), local(curve.subs((a + b) * 0.5)));
            SketchCurve::Arc {
                start,
                through,
                end,
            }
        };
        sketch.members.insert(
            member.clone(),
            cad_core::SketchMember {
                construction: true,
                curve: geometry,
            },
        );
        selected.push(member);
    }
    let operation = cad_core::sketch_edit::add_offset(
        &mut sketch,
        &selected,
        cad_core::Expr(format!("{} mm", offset.abs())),
        offset < 0.,
        false,
    )?;
    cad_core::sketch_edit::derive(doc, owner, &mut sketch)?;
    let derived_origins = sketch.offsets[&operation]
        .members
        .iter()
        .map(|member| {
            let index = member
                .source
                .strip_prefix("cap-edge-")
                .and_then(|index| index.parse::<usize>().ok())
                .ok_or_else(|| invalid("Derived cap contour lost source ancestry"))?;
            Ok((member.member.clone(), origins[index].clone()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    sketch
        .members
        .retain(|id, _| derived_origins.contains_key(id));
    sketch.offsets.clear();
    let built = crate::sketches::build_traced(doc, owner, &sketch, &frame)?;
    let Shape::Sketch { mut faces, .. } = built.shape else {
        return Err(invalid("Cap clearance must produce native sketch regions"));
    };
    if faces.len() != 1 {
        return Err(invalid("Cap clearance must leave one closed contour"));
    }
    let face = faces.remove(0);
    let ancestry = face
        .boundaries()
        .into_iter()
        .flatten()
        .map(|edge| {
            let (source, piece) = built
                .members
                .get(&edge.id())
                .ok_or_else(|| invalid("Sized cap edge ancestry is missing"))?;
            let original = &derived_origins[source];
            Ok(crate::section_history::Member {
                sources: original.sources.clone(),
                purpose: serde_json::json!({"source":original.purpose,"sized_piece":piece}),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((face, ancestry))
}
fn stock_rectangle(face: &Face) -> Result<([f64; 2], [f64; 3], bool)> {
    let frame = crate::adapter::cad_plane(face);
    let origin = Point3::from(frame.origin);
    let u = Vector3::from(frame.u);
    let v = Vector3::from(frame.v);
    let mut points = vec![];
    let mut exact = true;
    for edge in face.edge_iter() {
        let curve = edge.oriented_curve();
        let (a, b) = curve.range_tuple();
        points.extend([curve.subs(a), curve.subs(b)]);
        if matches!(curve, Curve::Line(_)) {
            continue;
        }
        if let Ok((center, axis, radius)) = crate::axis_geometry::circle_geometry(&curve) {
            let initial = curve.subs(a) - center;
            let sign = axis.dot(initial.cross(curve.der(a))).signum();
            if sign == 0. {
                return Err(invalid("Circular stock bound has no valid orientation"));
            }
            let angle = |radial: Vector3| {
                (sign * axis.dot(initial.cross(radial)))
                    .atan2(initial.dot(radial))
                    .rem_euclid(std::f64::consts::TAU)
            };
            let extent = if curve.subs(a).distance(curve.subs(b)) <= EPS {
                std::f64::consts::TAU
            } else {
                angle(curve.subs(b) - center)
            };
            for direction in [u, v] {
                let radial = direction - axis * direction.dot(axis);
                if radial.magnitude2() > 1e-18 {
                    for sign in [-1., 1.] {
                        let candidate = center + radial.normalize() * radius * sign;
                        if angle(candidate - center) <= extent + 1e-9 {
                            points.push(candidate);
                        }
                    }
                }
            }
        } else {
            exact = false;
            points.extend(crate::extrusion::curve_hull(&curve)?);
        }
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in points {
        let d = point - origin;
        for (i, value) in [d.dot(u), d.dot(v)].into_iter().enumerate() {
            min[i] = min[i].min(value);
            max[i] = max[i].max(value);
        }
    }
    let span = [max[0] - min[0], max[1] - min[1]];
    if span
        .iter()
        .any(|dimension| !dimension.is_finite() || *dimension <= EPS)
    {
        return Err(invalid("Accessory stock rectangle is empty or nonfinite"));
    }
    Ok((
        span,
        (origin + u * (max[0] + min[0]) * 0.5 + v * (max[1] + min[1]) * 0.5).into(),
        exact,
    ))
}
fn accessory_history(
    generated: &mut TopologyHistory,
    owner: &NodeRef,
    solid: &Solid,
    origins: &[crate::section_history::Member],
    context: serde_json::Value,
    section: &Face,
    direction: Vector3,
) -> Result<()> {
    let faces = solid.face_iter().collect::<Vec<_>>();
    if faces.len() != origins.len() + 2 {
        return Err(unsupported(
            "Accessory extrusion changed its native section ancestry",
        ));
    }
    let sources = origins
        .iter()
        .flat_map(|origin| origin.sources.clone())
        .collect::<Vec<_>>();
    let mut history = crate::section_history::History::new(owner, "weldment_accessory", generated);
    history.face(
        faces[0],
        &sources,
        serde_json::json!({"context":context,"end":"start"}),
    )?;
    history.face(
        faces.last().unwrap(),
        &sources,
        serde_json::json!({"context":context,"end":"finish"}),
    )?;
    let original = section
        .boundaries()
        .into_iter()
        .flatten()
        .zip(origins)
        .map(|(edge, origin)| (edge.id(), origin))
        .collect::<HashMap<_, _>>();
    let placed = if section.oriented_surface().normal(0., 0.).dot(direction) < 0. {
        section.inverse()
    } else {
        section.clone()
    };
    let ordered = placed
        .boundaries()
        .into_iter()
        .flatten()
        .map(|edge| original[&edge.id()])
        .collect::<Vec<_>>();
    for (face, origin) in faces.iter().skip(1).take(origins.len()).zip(ordered) {
        history.face(
            face,
            &origin.sources,
            serde_json::json!({"context":context,"wall":origin.purpose}),
        )?;
    }
    history.finish(solid)
}
fn build(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    stock: &Stock,
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Built> {
    let mut members = vec![];
    for definition in &stock.groups {
        members.extend(group(
            adapter,
            doc,
            definition,
            &stock.trims,
            inputs,
            generated,
        )?);
    }
    if stock.trims.iter().any(|trim| {
        trim.members
            .iter()
            .any(|reference| !members.iter().any(|member| member.reference == *reference))
    }) {
        return Err(invalid("Trim refers to a removed member group/segment"));
    }
    let mut extra = vec![];
    let mut accessories = vec![];
    for cap in &stock.caps {
        let member = members
            .iter()
            .find(|member| member.reference == cap.member)
            .ok_or_else(|| invalid("End-cap member was removed"))?;
        let (face, terminal, outward) = if cap.end == End::Start {
            (&member.start, member.start_terminal, -member.start_axis)
        } else {
            (&member.finish, member.end_terminal, member.end_axis)
        };
        if !terminal {
            return Err(unsupported(
                "Caps need terminal or explicitly trimmed open member ends",
            ));
        }
        if member.metadata.path_kind != MemberPathKind::Straight && cap.offset_mm < 0. {
            return Err(unsupported(
                "Recessed circular-member caps need a curved wall clearance construction; use a flush inner cap or an outward outer cap",
            ));
        }
        let boundaries = face.boundaries();
        let contour_index = if cap.contour == CapContour::Outer {
            0
        } else {
            1
        };
        let wire = match cap.contour {
            CapContour::Outer => boundaries
                .first()
                .cloned()
                .ok_or_else(|| invalid("Cap section has no outer contour"))?,
            CapContour::Inner => {
                if boundaries.len() != 2 {
                    return Err(invalid(
                        "Inner caps need exactly one hollow section contour",
                    ));
                }
                if cap.edge_offset_mm > EPS {
                    return Err(invalid(
                        "Inner-cap sizing must be zero or negative clearance to avoid overlapping the member wall",
                    ));
                }
                boundaries[1].clone()
            }
        };
        let mut n = face.oriented_surface().normal(0., 0.).normalize();
        if n.dot(outward) < 0. {
            n = -n;
        }
        if cap.offset_mm < 0. && n.cross(outward).magnitude() > 1e-8 {
            return Err(unsupported(
                "Recessed inner caps need a square member end; an angled pocket needs a matching wall clearance construction",
            ));
        }
        let (filled, origins) = sized_cap(
            doc,
            &cap.source.definition,
            face,
            wire,
            cap.edge_offset_mm,
            &member.origins[contour_index],
        )?;
        let filled = builder::translated(&filled, n * cap.offset_mm);
        if cap.offset_mm < 0. {
            let opposite = if cap.end == End::Start {
                &member.finish
            } else {
                &member.start
            };
            let opposite_outward = if cap.end == End::Start {
                member.end_axis
            } else {
                -member.start_axis
            };
            let surface = opposite.oriented_surface();
            let mut limit = surface.normal(0., 0.).normalize();
            if limit.dot(opposite_outward) < 0. {
                limit = -limit;
            }
            let anchor = surface.subs(0., 0.);
            if crate::extrusion::face_hull(&filled)?.iter().any(|point| {
                (*point - anchor).dot(limit) > EPS
                    || (*point + n * cap.thickness_mm - anchor).dot(limit) > EPS
            }) {
                return Err(invalid("Recessed cap crosses the opposite member end"));
            }
        }
        let (blank_size_mm, center, blank_bounds_exact) = stock_rectangle(&filled)?;
        let solid = crate::adapter::oriented_extrude(&filled, n * cap.thickness_mm);
        accessory_history(
            generated,
            &cap.source,
            &solid,
            &origins,
            serde_json::json!({"cap_member":cap.member,"contour":cap.contour,"end":cap.end}),
            &filled,
            n * cap.thickness_mm,
        )?;
        accessories.push(EvaluatedAccessory {
            reference: MemberRef {
                group: cap.source.clone(),
                member: "cap".into(),
            },
            kind: AccessoryKind::EndCap,
            profile: member.metadata.section.clone(),
            thickness_mm: cap.thickness_mm,
            blank_size_mm,
            center,
            blank_bounds_exact,
        });
        extra.push(solid);
    }
    for plate in &stock.plates {
        let (source, frame) = crate::extrusion::source(adapter, doc, &plate.profile, inputs)?;
        let traced =
            crate::sketches::build_traced(doc, &plate.profile.definition, &source, &frame)?;
        let Shape::Sketch { faces, .. } = traced.shape else {
            return Err(invalid("Gusset sketch has no closed regions"));
        };
        let n = normal(&frame)
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
        for face in faces {
            let outer = face
                .boundaries()
                .first()
                .cloned()
                .ok_or_else(|| invalid("Gusset region lost its outer contour"))?;
            let source = outer
                .iter()
                .filter_map(|edge| {
                    traced
                        .members
                        .get(&edge.id())
                        .map(|(member, _)| member.clone())
                })
                .min()
                .ok_or_else(|| invalid("Gusset outer contour has no persistent member identity"))?;
            let origins = face
                .boundaries()
                .into_iter()
                .flatten()
                .map(|edge| {
                    let (member, piece) = traced
                        .members
                        .get(&edge.id())
                        .ok_or_else(|| invalid("Gusset section ancestry is missing"))?;
                    crate::section_history::member(
                        &plate.profile,
                        &crate::extrusion::ThinMember {
                            sources: vec![member.clone()],
                            piece: *piece,
                            side: 0,
                            cap: false,
                        },
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let (blank_size_mm, center, blank_bounds_exact) = stock_rectangle(&face)?;
            accessories.push(EvaluatedAccessory {
                reference: MemberRef {
                    group: plate.source.clone(),
                    member: format!("gusset-contour-{source}"),
                },
                kind: AccessoryKind::Gusset,
                profile: plate.profile.clone(),
                thickness_mm: plate.thickness_mm,
                blank_size_mm,
                center,
                blank_bounds_exact,
            });
            let lower = builder::translated(&face, shift);
            let solid = crate::adapter::oriented_extrude(&lower, n * plate.thickness_mm);
            accessory_history(
                generated,
                &plate.source,
                &solid,
                &origins,
                serde_json::json!({"gusset_contour":source}),
                &lower,
                n * plate.thickness_mm,
            )?;
            extra.push(solid);
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
    Ok(Built {
        solids,
        metadata,
        accessories,
    })
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    Ok(Shape::Bodies(
        build(adapter, doc, &stock(doc, owner)?, inputs, generated)?.solids,
    ))
}
pub(crate) fn metadata(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Option<EvaluatedWeldment>> {
    if !doc.parts.get(&owner.definition).is_some_and(|part| {
        part.features.iter().any(|feature| {
            feature.id == owner.node
                && matches!(feature.operation, cad_core::Operation::Weldment { .. })
        })
    }) {
        return Ok(None);
    }
    let stock = stock(doc, owner)?;
    let built = build(
        adapter,
        doc,
        &stock,
        inputs,
        &mut TopologyHistory::default(),
    )?;
    Ok(Some(EvaluatedWeldment {
        version: CONTRACT_VERSION,
        source: owner.clone(),
        members: built.metadata,
        cap_count: stock.caps.len(),
        gusset_count: stock.plates.len(),
        accessories: built.accessories,
    }))
}
