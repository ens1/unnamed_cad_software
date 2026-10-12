//! Exact deposited weld metal from bounded native seam carriers.
//! The source path and support faces are resolved associatively; display meshes
//! never identify a joint or supply authoritative weld geometry.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Document, NodeRef, Result, TopologyRef,
    reference_curves::CurveSource,
    weld_beads::{
        CONTRACT_VERSION, ContactGuide, Definition, Evaluated, EvaluatedSegment, FaceSource,
        JOINT_PREVIEW_VERSION, JointPreview, MAX_SEGMENTS, Profile, SeamSource, Values,
    },
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap, HashSet};

const EPS: f64 = 1e-7;
const REL: f64 = 1e-10;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Weld bead: {}", message.to_string()))
}
fn unsupported(message: impl ToString) -> CadError {
    CadError::Unsupported(format!("Weld bead: {}", message.to_string()))
}
fn unit(vector: Vector3) -> Result<Vector3> {
    let length = vector.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Err(invalid("The seam or section has a degenerate direction"));
    }
    Ok(vector / length)
}

#[derive(Clone)]
pub(crate) struct ResolvedSeam {
    pub curves: Vec<Curve>,
    pub sources: Vec<TopologyRef>,
    pub adjacent_faces: Vec<(Face, TopologyRef)>,
    /// Native faces in the explicitly selected seam body's scope. A welded
    /// multibody stock may contact another member without sharing an edge ID.
    pub inference_faces: Vec<(Face, TopologyRef)>,
    pub inference_complete: bool,
}
impl ResolvedSeam {
    /// The assembly evaluator supplies the configured occurrence's rigid pose.
    pub(crate) fn transformed(&self, transform: Matrix4) -> Self {
        Self {
            curves: self
                .curves
                .iter()
                .map(|curve| curve.transformed(transform))
                .collect(),
            sources: self.sources.clone(),
            adjacent_faces: self
                .adjacent_faces
                .iter()
                .map(|(face, source)| (builder::transformed(face, transform), source.clone()))
                .collect(),
            inference_faces: self
                .inference_faces
                .iter()
                .map(|(face, source)| (builder::transformed(face, transform), source.clone()))
                .collect(),
            inference_complete: self.inference_complete,
        }
    }
}

pub(crate) fn seam(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    source: &SeamSource,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<ResolvedSeam> {
    let mut adjacent_faces = vec![];
    let mut inference_faces = vec![];
    let mut inference_complete = true;
    let pieces = match source {
        SeamSource::ModelEdge {
            body, reference, ..
        } => {
            let edges = adapter.selected_boundary_edges(
                body,
                std::slice::from_ref(reference.as_ref()),
                inputs,
            )?;
            if edges.len() != 1 {
                return Err(invalid(
                    "The saved model seam is missing or ambiguous; select an explicit replacement",
                ));
            }
            let edge = edges[0].absolute_clone();
            let actual = adapter.boundary_edge_reference(body, &edge)?;
            if !cad_core::provenance::equivalent(&actual, reference)? {
                return Err(invalid(
                    "The seam did not resolve by its saved construction ancestry",
                ));
            }
            let shape = inputs
                .get(body)
                .ok_or_else(|| invalid("The seam body is missing"))?;
            let mut seen = HashSet::new();
            for face in shape
                .boundary_shells()
                .ok_or_else(|| invalid("A model seam needs a solid or surface body"))?
                .into_iter()
                .flatten()
            {
                if inference_faces.len() < 512 {
                    inference_faces
                        .push((face.clone(), adapter.boundary_face_reference(body, face)?));
                } else {
                    inference_complete = false;
                }
                if face.edge_iter().any(|boundary| boundary.id() == edge.id())
                    && seen.insert(face.id())
                {
                    adjacent_faces
                        .push((face.clone(), adapter.boundary_face_reference(body, face)?));
                }
            }
            vec![(edge.oriented_curve(), actual)]
        }
        SeamSource::SketchMember {
            profile, member, ..
        } => crate::reference_curves::source(
            adapter,
            doc,
            &CurveSource::SketchMember {
                profile: profile.clone(),
                member: member.clone(),
            },
            inputs,
        )?,
        SeamSource::ReferenceCurve { feature, piece, .. } => {
            let Some(Shape::ReferenceCurves {
                paths, evaluated, ..
            }) = inputs.get(feature)
            else {
                return Err(invalid("The reusable seam curve is missing"));
            };
            if paths.len() != 1 {
                return Err(unsupported(
                    "A weld seam needs one connected reusable curve path",
                ));
            }
            let wanted = piece
                .as_ref()
                .map(|id| {
                    evaluated
                        .pieces
                        .iter()
                        .find(|piece| piece.id == *id)
                        .map(|piece| &piece.reference)
                        .ok_or_else(|| invalid("The saved reference seam piece was removed"))
                })
                .transpose()?;
            let wanted_identity = wanted
                .map(cad_core::provenance::normalized_reference)
                .transpose()?;
            let mut pieces = Vec::new();
            for edge in &paths[0] {
                let source = adapter.edge_source(feature, edge.id());
                if let Some(wanted_identity) = &wanted_identity {
                    let Some(source) = &source else {
                        continue;
                    };
                    if cad_core::provenance::normalized_reference(source)? != *wanted_identity {
                        continue;
                    }
                }
                let source = source
                    .ok_or_else(|| invalid("The seam curve has no saved construction ancestry"))?;
                pieces.push((edge.oriented_curve(), source));
            }
            pieces
        }
    };
    if pieces.is_empty() || pieces.len() > 4096 {
        return Err(invalid("A seam needs 1–4096 bounded native curve pieces"));
    }
    let (curves, sources) = pieces.into_iter().unzip();
    Ok(ResolvedSeam {
        curves,
        sources,
        adjacent_faces,
        inference_faces,
        inference_complete,
    })
}
pub(crate) fn support(
    adapter: &MonstertruckAdapter,
    source: &FaceSource,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<(Face, TopologyRef)> {
    let face = adapter.selected_boundary_face(&source.body, &source.reference, inputs)?;
    let ancestry = adapter.boundary_face_reference(&source.body, &face)?;
    Ok((face, ancestry))
}

enum Path {
    Line {
        start: Point3,
        tangent: Vector3,
        length: f64,
    },
    Circle {
        center: Point3,
        axis: Vector3,
        radial: Vector3,
        radius: f64,
        angle: f64,
    },
}
impl Path {
    fn length(&self) -> f64 {
        match self {
            Self::Line { length, .. } => *length,
            Self::Circle { radius, angle, .. } => radius * angle,
        }
    }
    fn point(&self, position: f64) -> Point3 {
        match self {
            Self::Line { start, tangent, .. } => *start + *tangent * position,
            Self::Circle {
                center,
                axis,
                radial,
                radius,
                ..
            } => {
                *center
                    + Matrix3::from_axis_angle(*axis, Rad(position / radius)) * (*radial * *radius)
            }
        }
    }
    fn tangent(&self, position: f64) -> Vector3 {
        match self {
            Self::Line { tangent, .. } => *tangent,
            Self::Circle {
                axis,
                radial,
                radius,
                ..
            } => axis.cross(Matrix3::from_axis_angle(*axis, Rad(position / radius)) * radial),
        }
    }
    fn rotation(&self, position: f64) -> Matrix3 {
        match self {
            Self::Line { .. } => Matrix3::identity(),
            Self::Circle { axis, radius, .. } => {
                Matrix3::from_axis_angle(*axis, Rad(position / radius))
            }
        }
    }
}
fn classify(curves: &[Curve]) -> Result<Path> {
    let first = curves.first().ok_or_else(|| invalid("The seam is empty"))?;
    let (a, b) = first.range_tuple();
    if !a.is_finite() || !b.is_finite() || b <= a {
        return Err(invalid(
            "The seam has no bounded increasing native parameter",
        ));
    }
    let start = first.subs(a);
    let end = first.subs(b);
    let finite = |p: Point3| {
        [p.x, p.y, p.z]
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1e12)
    };
    if !finite(start) || !finite(end) {
        return Err(invalid("Seam endpoints exceed finite document coordinates"));
    }
    if crate::bounded_carriers::line(first).is_ok() {
        let tangent = unit(end - start)?;
        let mut cursor = start;
        let mut length = 0.;
        for curve in curves {
            crate::bounded_carriers::line(curve).map_err(|_| unsupported("A seam chain must be wholly collinear or wholly circular; transported spline and corner welds need separate seams"))?;
            let (a, b) = curve.range_tuple();
            let p = curve.subs(a);
            let q = curve.subs(b);
            if !finite(p) || !finite(q) {
                return Err(invalid("Seam endpoints exceed finite document coordinates"));
            }
            let span = (q - p).dot(tangent);
            if p.distance(cursor) > EPS || (q - p).cross(tangent).magnitude() > EPS || span <= EPS {
                return Err(invalid(
                    "The seam chain has a gap, reversal, or a change of direction",
                ));
            }
            length += span;
            cursor = q;
        }
        if !length.is_finite() || length > 1e12 {
            return Err(invalid("Seam length exceeds finite document coordinates"));
        }
        return Ok(Path::Line {
            start,
            tangent,
            length,
        });
    }
    let ((center,normal,radius),sweep)=crate::bounded_carriers::circle(first).map_err(|_| unsupported("Visible native beads currently need proven straight or circular seam carriers; general spline transport is unresolved"))?;
    if !finite(center) || !radius.is_finite() || radius > 1e12 {
        return Err(invalid(
            "The seam's circle exceeds finite document coordinates",
        ));
    }
    let axis = normal * sweep.signum();
    let radial = unit(start - center)?;
    let mut cursor = start;
    let mut angle = 0.;
    for curve in curves {
        let ((c, n, r), s) = crate::bounded_carriers::circle(curve)?;
        let (a, b) = curve.range_tuple();
        let p = curve.subs(a);
        let q = curve.subs(b);
        if !finite(p) || !finite(q) {
            return Err(invalid("Seam endpoints exceed finite document coordinates"));
        }
        if p.distance(cursor) > EPS
            || c.distance(center) > EPS
            || (r - radius).abs() > EPS
            || n.dot(axis) * s.signum() < 1. - REL
        {
            return Err(invalid(
                "Circular seam pieces need a connected coaxial carrier and consistent traversal",
            ));
        }
        angle += s.abs();
        cursor = q;
    }
    if !angle.is_finite() || angle <= REL || angle > std::f64::consts::TAU + REL {
        return Err(invalid(
            "A circular seam cannot collapse or retrace more than one revolution",
        ));
    }
    Ok(Path::Circle {
        center,
        axis,
        radial,
        radius,
        angle: angle.min(std::f64::consts::TAU),
    })
}

struct Section {
    face: Face,
    wall_roles: HashMap<EdgeId, &'static str>,
    area: f64,
    centroid: Vector3,
    toes: Vec<Vector3>,
}
fn section(
    definition: &Definition,
    values: &Values,
    origin: Point3,
    tangent: Vector3,
    u: Vector3,
    v: Vector3,
) -> Result<Section> {
    let size = values.size_mm;
    let rise = values.reinforcement_mm;
    let (a, b, triangle_area, triangle_centroid, toes) = match definition.profile {
        Profile::Fillet => {
            let a = u * size;
            let b = v * size;
            (a, b, a.cross(b).magnitude() * 0.5, (a + b) / 3., vec![a, b])
        }
        Profile::Butt => (
            -u * (size * 0.5),
            u * (size * 0.5),
            0.,
            Vector3::zero(),
            vec![-u * (size * 0.5), u * (size * 0.5)],
        ),
    };
    let chord = (b - a).magnitude();
    let outward = match definition.profile {
        Profile::Fillet => unit(u + v)?,
        Profile::Butt => v,
    };
    if rise > chord * 0.5 + EPS {
        return Err(invalid(
            "Crown rise cannot exceed half of the toe-to-toe chord; larger crowns fold the contact section",
        ));
    }
    let mut edges = vec![];
    let mut roles = vec![];
    let av = builder::vertex(origin + a);
    let bv = builder::vertex(origin + b);
    if definition.profile == Profile::Fillet {
        let root = builder::vertex(origin);
        edges.push(builder::line(&root, &av));
        roles.push("first_leg");
        edges.push(if rise <= EPS {
            builder::line(&av, &bv)
        } else {
            builder::try_circle_arc(&av, &bv, origin + (a + b) * 0.5 + outward * rise)
                .map_err(invalid)?
        });
        roles.push("crown");
        edges.push(builder::line(&bv, &root));
        roles.push("second_leg");
    } else {
        edges.push(builder::try_circle_arc(&av, &bv, origin + outward * rise).map_err(invalid)?);
        roles.push("crown");
        edges.push(builder::line(&bv, &av));
        roles.push("root_contact");
    }
    let mut face: Face =
        builder::try_attach_plane(vec![Wire::from(edges.clone())]).map_err(invalid)?;
    if face.oriented_surface().normal(0., 0.).dot(tangent) < 0. {
        face.invert();
    }
    let (cap_area, cap_centroid) = if rise <= EPS {
        (0., Vector3::zero())
    } else {
        let radius = chord * chord / (8. * rise) + rise * 0.5;
        let angle = 4. * (2. * rise / chord).atan();
        let difference = angle - angle.sin();
        let area = radius * radius * difference * 0.5;
        // Thin circular segments need cancellation-resistant positive area.
        if !area.is_finite() || area <= 1e-16 || difference <= 0. {
            return Err(unsupported(
                "The crown is too shallow to retain bounded native area at this scale",
            ));
        }
        let distance =
            4. * radius * (angle * 0.5).sin().powi(3) / (3. * difference) - (radius - rise);
        (area, (a + b) * 0.5 + outward * distance)
    };
    let area = triangle_area + cap_area;
    if !area.is_finite() || area <= 1e-14 {
        return Err(invalid("Weld section area collapses"));
    }
    Ok(Section {
        face,
        wall_roles: edges
            .into_iter()
            .zip(roles)
            .map(|(edge, role)| (edge.id(), role))
            .collect(),
        area,
        centroid: (triangle_centroid * triangle_area + cap_centroid * cap_area) / area,
        toes,
    })
}

struct Support {
    face: Face,
    carrier: crate::analytic_offset::Carrier,
    side: f64,
}
fn supports(faces: &[(Face, TopologyRef)]) -> Result<Vec<Support>> {
    faces.iter().map(|(face,_)| {
        let carrier=crate::analytic_offset::recognize(face)?.ok_or_else(|| unsupported("A joint support requires a proven plane or cylinder carrier"))?;
        if !matches!(carrier,crate::analytic_offset::Carrier::Plane{..}|crate::analytic_offset::Carrier::Cone{slope:0.,..}) {return Err(unsupported("Fillet contact supports currently require planes or exact cylinders; conical/freeform toes need a different contact profile"));}
        Ok(Support{face:face.clone(),carrier,side:crate::analytic_offset::side(face,carrier)?})
    }).collect()
}
fn frame(
    definition: &Definition,
    values: &Values,
    path: &Path,
    supports: &[Support],
) -> Result<(Vector3, Vector3)> {
    let origin = path.point(0.);
    let tangent = path.tangent(0.);
    if !supports.is_empty() {
        if values.roll_rad.abs() > REL {
            return Err(invalid(
                "A face-supported weld follows its joint; roll applies only to explicitly authored unsupported seam paths",
            ));
        }
        let normals = supports
            .iter()
            .map(|support| Ok(support.carrier.normal(origin)? * support.side))
            .collect::<Result<Vec<_>>>()?;
        match definition.profile {
            Profile::Fillet => {
                if normals.len() != 2 {
                    return Err(invalid(
                        "The model edge needs two joint faces, or select two explicit supports",
                    ));
                }
                let n = normals[0];
                let m = normals[1];
                if n.dot(tangent).abs() > REL
                    || m.dot(tangent).abs() > REL
                    || n.cross(m).magnitude() <= 1e-8
                {
                    return Err(invalid(
                        "The selected joint faces are parallel or do not meet along the seam",
                    ));
                }
                let u = unit(m - n * n.dot(m))?;
                let v = unit(n - m * m.dot(n))?;
                match path {
                    Path::Line { .. }
                        if supports.iter().any(|support| {
                            !matches!(
                                support.carrier,
                                crate::analytic_offset::Carrier::Plane { .. }
                            )
                        }) =>
                    {
                        return Err(unsupported(
                            "Straight fillet joints require planar contact faces; a cylinder's longitudinal contact needs a curved toe section",
                        ));
                    }
                    Path::Circle {
                        center,
                        axis,
                        radius,
                        ..
                    } => {
                        let mut planes = 0;
                        let mut cylinders = 0;
                        for support in supports {
                            match support.carrier {
                                crate::analytic_offset::Carrier::Plane { origin, normal }
                                    if normal.dot(*axis).abs() > 1. - REL
                                        && (origin - *center).dot(normal).abs() <= EPS =>
                                {
                                    planes += 1
                                }
                                crate::analytic_offset::Carrier::Cone {
                                    origin,
                                    axis: a,
                                    radius: r,
                                    slope: 0.,
                                } if a.dot(*axis).abs() > 1. - REL
                                    && (origin - *center).cross(*axis).magnitude() <= EPS
                                    && (r - *radius).abs() <= EPS =>
                                {
                                    cylinders += 1
                                }
                                _ => {
                                    return Err(unsupported(
                                        "Circular fillet joints need one transverse plane and one coaxial cylindrical wall at the seam radius",
                                    ));
                                }
                            }
                        }
                        if planes != 1 || cylinders != 1 {
                            return Err(unsupported(
                                "Circular fillet support requires a plane-to-cylinder joint",
                            ));
                        }
                    }
                    _ => {}
                }
                Ok((u, v))
            }
            Profile::Butt => {
                if !(1..=2).contains(&normals.len()) {
                    return Err(invalid(
                        "A butt bead needs one planar support or two coplanar joint faces",
                    ));
                }
                let v = unit(normals[0] - tangent * normals[0].dot(tangent))?;
                if supports.iter().any(|support| {
                    !matches!(
                        support.carrier,
                        crate::analytic_offset::Carrier::Plane { .. }
                    )
                }) {
                    return Err(unsupported(
                        "Butt bead width currently requires a planar root support",
                    ));
                }
                if normals.len() == 2
                    && (normals[0].dot(normals[1]) < 1. - REL
                        || normals[1].dot(tangent).abs() > REL)
                {
                    return Err(invalid(
                        "Two-face butt supports must be coplanar with matching outward normals; opposing end faces need a groove preparation",
                    ));
                }
                Ok((unit(tangent.cross(v))?, v))
            }
        }
    } else {
        let direction = Vector3::from(definition.direction);
        let perpendicular = unit(direction - tangent * direction.dot(tangent))?;
        let roll = Matrix3::from_axis_angle(tangent, Rad(values.roll_rad));
        match definition.profile {
            Profile::Fillet => {
                let u = roll * perpendicular;
                Ok((u, tangent.cross(u)))
            }
            Profile::Butt => {
                let v = roll * perpendicular;
                Ok((tangent.cross(v), v))
            }
        }
    }
}

fn planar_contact(
    support: &Support,
    path: &Path,
    offset: Vector3,
    start: f64,
    end: f64,
) -> Result<()> {
    use crate::join::region::{Frame, Location, checked_intersections, locate};
    let crate::analytic_offset::Carrier::Plane { origin, normal } = support.carrier else {
        return Err(invalid("Expected a planar toe support"));
    };
    let x = unit(normal.cross(if normal.x.abs() < 0.8 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    }))?;
    let frame = Frame {
        origin,
        axes: [x, normal.cross(x)],
        normal,
    };
    let boundaries = support.face.boundaries();
    if boundaries.len() > 1024 || boundaries.iter().map(|wire| wire.len()).sum::<usize>() > 16384 {
        return Err(unsupported(
            "Contact boundary exceeds the native proof budget",
        ));
    }
    let region = boundaries
        .iter()
        .flat_map(|wire| wire.iter())
        .map(|edge| {
            frame.segment(edge)?.ok_or_else(|| {
                unsupported("Planar toe containment needs line/circle support boundaries")
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let point = |s: f64| path.point(s) + path.rotation(s) * offset;
    let p = point(start);
    let q = point(end);
    if (p - origin).dot(normal).abs() > EPS || (q - origin).dot(normal).abs() > EPS {
        return Err(invalid("The seam or bead toe is not on its support face"));
    }
    let av = builder::vertex(p);
    let bv = builder::vertex(q);
    // Split a full turn into bounded arcs to retain unambiguous native intervals.
    let spans = match path {
        Path::Line { .. } => 1,
        Path::Circle { radius, .. } => ((end - start) / radius / std::f64::consts::FRAC_PI_2)
            .ceil()
            .max(1.) as usize,
    };
    for span in 0..spans {
        let a = start + (end - start) * span as f64 / spans as f64;
        let b = start + (end - start) * (span + 1) as f64 / spans as f64;
        let (av, bv) = if spans == 1 {
            (av.clone(), bv.clone())
        } else {
            (builder::vertex(point(a)), builder::vertex(point(b)))
        };
        let edge = match path {
            Path::Line { .. } => builder::line(&av, &bv),
            Path::Circle { .. } => {
                builder::try_circle_arc(&av, &bv, point((a + b) * 0.5)).map_err(invalid)?
            }
        };
        let segment = frame
            .segment(&edge)?
            .ok_or_else(|| unsupported("The toe has no exact planar carrier"))?;
        let mut partitions = vec![0., 1.];
        for boundary in &region {
            for crossing in checked_intersections(&segment, boundary)? {
                let fraction = match path {
                    Path::Line { .. } => {
                        let p = frame.lift(crossing);
                        let d = edge.back().point() - edge.front().point();
                        (p - edge.front().point()).dot(d) / d.magnitude2()
                    }
                    Path::Circle { axis, center, .. } => {
                        let first = edge.front().point() - *center;
                        let hit = frame.lift(crossing) - *center;
                        let angle = axis
                            .dot(first.cross(hit))
                            .atan2(first.dot(hit))
                            .rem_euclid(std::f64::consts::TAU);
                        angle
                            / ((b - a)
                                / match path {
                                    Path::Circle { radius, .. } => *radius,
                                    _ => unreachable!(),
                                })
                    }
                };
                if (0. ..=1.).contains(&fraction) {
                    partitions.push(fraction);
                }
            }
        }
        partitions.sort_by(f64::total_cmp);
        partitions.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        for t in [0., 1.].into_iter().chain(
            partitions
                .windows(2)
                .map(|range| (range[0] + range[1]) * 0.5),
        ) {
            if locate(segment.at(t), &region)? == Location::Outside {
                return Err(invalid(
                    "The bead toe extends outside the selected support's trimmed boundaries; reduce bead size, increase start/end trimming where the toe crosses an end, or replace the support",
                ));
            }
        }
    }
    Ok(())
}

fn cylindrical_contact(
    support: &Support,
    path: &Path,
    offset: Vector3,
    start: f64,
    end: f64,
) -> Result<()> {
    let crate::analytic_offset::Carrier::Cone {
        origin,
        axis,
        radius,
        slope: 0.,
    } = support.carrier
    else {
        return Err(invalid("Expected an exact cylinder support"));
    };
    let Path::Circle {
        center,
        axis: seam_axis,
        radial,
        radius: seam_radius,
        ..
    } = path
    else {
        return Err(unsupported(
            "Cylindrical bead contacts require a circular seam",
        ));
    };
    if axis.dot(*seam_axis).abs() < 1. - REL
        || (radius - *seam_radius).abs() > EPS
        || (origin - *center).cross(axis).magnitude() > EPS
    {
        return Err(invalid(
            "The cylindrical support does not share the seam's axis and radius",
        ));
    }
    if offset.cross(axis).magnitude() > EPS {
        return Err(unsupported(
            "A cylindrical weld toe must run axially along the wall",
        ));
    }
    let z = (path.point(start) + path.rotation(start) * offset - origin).dot(axis);
    let boundaries = support.face.boundaries();
    if boundaries.len() != 1 || boundaries[0].len() != 4 {
        return Err(unsupported(
            "Cylindrical toe containment currently requires one exact four-edge wall patch; subdivided/imported wall trims need expanded angular arrangements",
        ));
    }
    let mut circular = vec![];
    let mut longitudinal = vec![];
    for edge in &boundaries[0] {
        let curve = edge.oriented_curve();
        let (a, b) = curve.range_tuple();
        if let Ok(((c, n, r), sweep)) = crate::bounded_carriers::circle(&curve) {
            if (c - origin).cross(axis).magnitude() > EPS
                || n.dot(axis).abs() < 1. - REL
                || (r - radius).abs() > EPS
            {
                return Err(invalid("A cylindrical support boundary is not coaxial"));
            }
            let radial_a = unit(curve.subs(a) - c)?;
            let angle = seam_axis
                .dot(radial.cross(radial_a))
                .atan2(radial.dot(radial_a));
            let amount = sweep * n.dot(*seam_axis).signum();
            circular.push(((c - origin).dot(axis), angle, angle + amount));
        } else {
            crate::bounded_carriers::line(&curve)?;
            let p = curve.subs(a);
            let q = curve.subs(b);
            if (q - p).cross(axis).magnitude() > EPS {
                return Err(unsupported(
                    "A cylindrical contact trim must have axial straight sides",
                ));
            }
            longitudinal.push((p, q));
        }
    }
    if circular.len() != 2 || longitudinal.len() != 2 {
        return Err(unsupported(
            "A cylindrical contact needs two circular axial limits and two straight angular limits",
        ));
    }
    let z_min = circular[0].0.min(circular[1].0);
    let z_max = circular[0].0.max(circular[1].0);
    if z < z_min - EPS || z > z_max + EPS {
        return Err(invalid(
            "The cylindrical toe exceeds the support's axial trim",
        ));
    }
    let requested = [start / seam_radius, end / seam_radius];
    let intervals = circular
        .iter()
        .map(|(_, a, b)| [a.min(*b), a.max(*b)])
        .collect::<Vec<_>>();
    for turns in -2..=2 {
        let shift = turns as f64 * std::f64::consts::TAU;
        let a = intervals[0][0] + shift;
        let b = intervals[0][1] + shift;
        let other = intervals[1];
        let same = (-2..=2).any(|turns| {
            let delta = turns as f64 * std::f64::consts::TAU;
            (a - other[0] - delta).abs() * radius <= EPS
                && (b - other[1] - delta).abs() * radius <= EPS
        });
        if same && requested[0] >= a - EPS / radius && requested[1] <= b + EPS / radius {
            // Native rectangular boundary identity: both axial sides terminate
            // on those two circular limits at the matching angular endpoints.
            for (p, q) in &longitudinal {
                let za = (*p - origin).dot(axis);
                let zb = (*q - origin).dot(axis);
                if (za.min(zb) - z_min).abs() > EPS || (za.max(zb) - z_max).abs() > EPS {
                    return Err(unsupported(
                        "Cylindrical boundary sides do not close the same axial patch",
                    ));
                }
                let r = unit(*p - origin - axis * za)?;
                let angle = seam_axis.dot(radial.cross(r)).atan2(radial.dot(r));
                if ((a - angle) * 0.5)
                    .sin()
                    .abs()
                    .min(((b - angle) * 0.5).sin().abs())
                    > EPS / radius
                {
                    return Err(unsupported(
                        "Cylindrical side boundaries do not close the same angular patch",
                    ));
                }
            }
            return Ok(());
        }
    }
    Err(invalid(
        "The cylindrical bead toe extends outside the selected wall patch",
    ))
}
fn contact(support: &Support, path: &Path, offset: Vector3, start: f64, end: f64) -> Result<()> {
    match support.carrier {
        crate::analytic_offset::Carrier::Plane { .. } => {
            planar_contact(support, path, offset, start, end)
        }
        crate::analytic_offset::Carrier::Cone { slope: 0., .. } => {
            cylindrical_contact(support, path, offset, start, end)
        }
        _ => Err(unsupported("The joint contact carrier is unresolved")),
    }
}

/// Prove the complete planar deposited leg/root footprint, including transverse
/// ends and interior cutouts. Root and toe curves alone do not exclude a hole
/// lying entirely between them. Display tessellation never supplies this proof.
fn footprint(support: &Support, path: &Path, toe: Vector3, start: f64, end: f64) -> Result<()> {
    contact(support, path, Vector3::zero(), start, end)?;
    contact(support, path, toe, start, end)?;
    let crate::analytic_offset::Carrier::Plane { origin, normal } = support.carrier else {
        // Circular fillet contacts on cylinders have axial straight legs. The
        // existing exact rectangular angular/axial patch proof covers that strip.
        return Ok(());
    };
    if toe.magnitude() <= EPS {
        return Ok(());
    }
    use crate::join::region::{Frame, Location, checked_intersections, locate};
    let x = unit(normal.cross(if normal.x.abs() < 0.8 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    }))?;
    let frame = Frame {
        origin,
        axes: [x, normal.cross(x)],
        normal,
    };
    let full = matches!(path, Path::Circle { radius, .. } if ((end-start)/radius-std::f64::consts::TAU).abs() <= REL);
    let spans = match path {
        Path::Line { .. } => 1,
        Path::Circle { radius, .. } => ((end - start) / radius / std::f64::consts::FRAC_PI_2)
            .ceil()
            .max(1.) as usize,
    };
    let point =
        |position: f64, offset: Vector3| path.point(position) + path.rotation(position) * offset;
    let mut ribbon = vec![];
    for offset in [Vector3::zero(), toe] {
        for span in 0..spans {
            let a = start + (end - start) * span as f64 / spans as f64;
            let b = start + (end - start) * (span + 1) as f64 / spans as f64;
            let av = builder::vertex(point(a, offset));
            let bv = builder::vertex(point(b, offset));
            let edge = match path {
                Path::Line { .. } => builder::line(&av, &bv),
                Path::Circle { .. } => {
                    builder::try_circle_arc(&av, &bv, point((a + b) * 0.5, offset))
                        .map_err(invalid)?
                }
            };
            ribbon.push(
                frame.segment(&edge)?.ok_or_else(|| {
                    unsupported("The contact footprint has no exact planar carrier")
                })?,
            );
        }
    }
    if !full {
        for position in [start, end] {
            let p = path.point(position);
            let offset = path.rotation(position) * toe;
            let length = offset.magnitude();
            let crossing = Path::Line {
                start: p,
                tangent: unit(offset)?,
                length,
            };
            contact(support, &crossing, Vector3::zero(), 0., length)?;
            let edge = builder::line(&builder::vertex(p), &builder::vertex(p + offset));
            ribbon.push(
                frame
                    .segment(&edge)?
                    .ok_or_else(|| unsupported("The contact end has no exact planar carrier"))?,
            );
        }
    }
    for edge in support.face.edge_iter() {
        let boundary = frame.segment(&edge)?.ok_or_else(|| {
            unsupported("Complete contact containment needs line/circle support boundaries")
        })?;
        let mut partitions = vec![0., 1.];
        for side in &ribbon {
            for hit in checked_intersections(&boundary, side)? {
                partitions.push(boundary.fraction(hit).ok_or_else(|| {
                    unsupported(
                        "A footprint intersection could not retain its bounded native interval",
                    )
                })?);
            }
        }
        partitions.sort_by(f64::total_cmp);
        partitions.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        for range in partitions.windows(2) {
            if locate(boundary.at((range[0] + range[1]) * 0.5), &ribbon)? == Location::Inside {
                return Err(invalid(
                    "The deposited contact footprint crosses a support cutout or trim boundary; replace the support or reduce bead size/end extent",
                ));
            }
        }
    }
    Ok(())
}

struct ContactBudget {
    remaining: usize,
}
impl ContactBudget {
    fn new() -> Self {
        Self {
            remaining: 2_000_000,
        }
    }
    fn charge(&mut self, support: &Support, path: &Path, spans: &[[f64; 2]]) -> Result<()> {
        let boundaries = support
            .face
            .boundaries()
            .iter()
            .map(|wire| wire.len())
            .sum::<usize>();
        let divisions = spans
            .iter()
            .map(|[start, end]| match path {
                Path::Line { .. } => 1,
                Path::Circle { radius, .. } => {
                    ((end - start) / radius / std::f64::consts::FRAC_PI_2)
                        .ceil()
                        .max(1.) as usize
                }
            })
            .sum::<usize>();
        let cost = boundaries
            .checked_mul(divisions)
            .and_then(|n| n.checked_mul(16))
            .ok_or_else(|| unsupported("The joint contact proof exceeds its bounded work"))?;
        self.remaining=self.remaining.checked_sub(cost).ok_or_else(||unsupported("The joint contact proof exceeds its bounded work; narrow the seam or select explicit supports"))?;
        Ok(())
    }
}

/// Butt contacts on two coplanar stock faces occupy opposite halves of the
/// section. Prefer the first ordering only if the complete native footprints
/// pass; a failed ordering remains visible in the ordinary per-toe diagnostics.
fn support_toes(
    profile: Profile,
    supports: &[Support],
    path: &Path,
    toes: &[Vector3],
    spans: &[[f64; 2]],
    budget: &mut ContactBudget,
) -> Result<Vec<Vec<(usize, Vector3)>>> {
    if profile == Profile::Fillet {
        return Ok(toes
            .iter()
            .copied()
            .enumerate()
            .take(supports.len())
            .map(|(index, toe)| vec![(index, toe)])
            .collect());
    }
    if supports.len() == 1 {
        return Ok(vec![toes.iter().copied().enumerate().collect()]);
    }
    let mut assignment = vec![vec![(0, toes[0])], vec![(1, toes[1])]];
    let mut pass = |first: usize| -> Result<bool> {
        for (support, index) in [(0, first), (1, 1 - first)] {
            budget.charge(&supports[support], path, spans)?;
            for [start, end] in spans {
                match footprint(&supports[support], path, toes[index], *start, *end) {
                    Ok(()) => {}
                    Err(CadError::Unsupported(message)) => {
                        return Err(CadError::Unsupported(message));
                    }
                    Err(_) => return Ok(false),
                }
            }
        }
        Ok(true)
    };
    if !pass(0)? {
        let mut reversed = pass(1)?;
        if !reversed {
            // A unique miter joint can have a valid middle but require end
            // trimming. Use that native interior contact only to orient its
            // diagnostic toes, never to accept the complete footprint.
            if let Some([start, end]) = spans
                .iter()
                .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
            {
                let middle = (start + end) * 0.5;
                let half = (end - start) * 0.125;
                let interior = [[middle - half, middle + half]];
                for support in supports {
                    budget.charge(support, path, &interior)?;
                }
                reversed = footprint(&supports[0], path, toes[1], interior[0][0], interior[0][1])
                    .is_ok()
                    && footprint(&supports[1], path, toes[0], interior[0][0], interior[0][1])
                        .is_ok();
            }
        }
        if reversed {
            assignment = vec![vec![(1, toes[1])], vec![(0, toes[0])]];
        }
    }
    Ok(assignment)
}

fn inferred_joint(
    definition: &Definition,
    values: &Values,
    resolved: &ResolvedSeam,
    path: &Path,
    budget: &mut ContactBudget,
) -> Result<Vec<(Face, TopologyRef)>> {
    if !resolved.inference_complete {
        return Err(unsupported(
            "The selected seam body exceeds the bounded joint inference face budget; choose explicit support faces",
        ));
    }
    let spans = intervals(values, path.length())?;
    let mut candidates = vec![];
    for face in &resolved.inference_faces {
        let Ok(mut carriers) = supports(std::slice::from_ref(face)) else {
            continue;
        };
        let support = carriers.remove(0);
        let root_carrier = match (support.carrier, path) {
            (
                crate::analytic_offset::Carrier::Plane { origin, normal },
                Path::Line {
                    start,
                    tangent,
                    length,
                },
            ) => {
                (*start - origin).dot(normal).abs() <= EPS
                    && tangent.dot(normal).abs() * length <= EPS
            }
            (
                crate::analytic_offset::Carrier::Plane { origin, normal },
                Path::Circle { center, axis, .. },
            ) => (*center - origin).dot(normal).abs() <= EPS && normal.dot(*axis).abs() > 1. - REL,
            (
                crate::analytic_offset::Carrier::Cone {
                    origin,
                    axis,
                    radius,
                    slope: 0.,
                },
                Path::Circle {
                    center,
                    axis: seam_axis,
                    radius: seam_radius,
                    ..
                },
            ) => {
                axis.dot(*seam_axis).abs() > 1. - REL
                    && (origin - *center).cross(axis).magnitude() <= EPS
                    && (radius - seam_radius).abs() <= EPS
            }
            _ => false,
        };
        if !root_carrier {
            continue;
        }
        budget.charge(&support, path, &spans)?;
        let mut root_contact = true;
        for [start, end] in &spans {
            match contact(&support, path, Vector3::zero(), *start, *end) {
                Ok(()) => {}
                Err(CadError::Unsupported(message)) => return Err(CadError::Unsupported(message)),
                Err(_) => {
                    root_contact = false;
                    break;
                }
            }
        }
        if root_contact {
            candidates.push((face.clone(), support));
            if candidates.len() > 32 {
                return Err(unsupported(
                    "The seam has too many contacting faces for unique inference; select explicit support faces",
                ));
            }
        }
    }
    let mut matches = vec![];
    let mut diagnostic_butt_joints = vec![];
    for first in 0..candidates.len() {
        let seconds = if definition.profile == Profile::Butt {
            first..candidates.len()
        } else {
            first + 1..candidates.len()
        };
        for second in seconds {
            let single = first == second;
            let faces = if single {
                vec![candidates[first].0.clone()]
            } else {
                vec![candidates[first].0.clone(), candidates[second].0.clone()]
            };
            let native = supports(&faces)?;
            let Ok((u, v)) = frame(definition, values, path, &native) else {
                continue;
            };
            if definition.profile == Profile::Butt && !single {
                diagnostic_butt_joints.push(faces.clone());
            }
            let toes = if definition.profile == Profile::Fillet {
                vec![u * values.size_mm, v * values.size_mm]
            } else {
                vec![-u * (values.size_mm * 0.5), u * (values.size_mm * 0.5)]
            };
            let assignments =
                support_toes(definition.profile, &native, path, &toes, &spans, budget)?;
            let mut valid = true;
            for (support, assigned) in native.iter().zip(assignments) {
                budget.charge(support, path, &spans)?;
                for (_, toe) in assigned {
                    for [start, end] in &spans {
                        match footprint(support, path, toe, *start, *end) {
                            Ok(()) => {}
                            Err(CadError::Unsupported(message)) => {
                                return Err(CadError::Unsupported(message));
                            }
                            Err(_) => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if !valid {
                        break;
                    }
                }
                if !valid {
                    break;
                }
            }
            if valid {
                matches.push(faces);
            }
            if matches.len() > 1 {
                return Err(invalid(
                    "Several complete native support joints fit this seam; select explicit support faces instead of guessing",
                ));
            }
        }
    }
    if matches.is_empty() && diagnostic_butt_joints.len() == 1 {
        // Root contact and coplanar orientation are proved, but no deposited
        // solid is accepted until the full toes/ends pass below. Showing this
        // unique candidate exposes the required end trim instead of reverting
        // to the unrelated convex faces adjacent to the picked edge.
        return Ok(diagnostic_butt_joints.remove(0));
    }
    if matches.is_empty() && diagnostic_butt_joints.len() > 1 {
        return Err(invalid(
            "Several coplanar butt joints share the seam but none has complete footprint contact; pick explicit support faces and end trims",
        ));
    }
    // Preserve the original adjacent-face attempt when no complete inferred
    // joint exists: its seam/toe curves explain the rejection and recovery.
    Ok(matches
        .pop()
        .unwrap_or_else(|| resolved.adjacent_faces.clone()))
}
fn intervals(values: &Values, length: f64) -> Result<Vec<[f64; 2]>> {
    let first = values.start_offset_mm;
    let last = length - values.end_offset_mm;
    if !last.is_finite() || last - first <= EPS {
        return Err(invalid("The start and end offsets consume the entire seam"));
    }
    let Some([stitch, gap]) = values.stitch_mm else {
        return Ok(vec![[first, last]]);
    };
    let count = ((last - first) / (stitch + gap)).ceil() as usize;
    if count > MAX_SEGMENTS {
        return Err(unsupported(
            "The intermittent bead exceeds 1024 deposited segments; increase stitch spacing",
        ));
    }
    let mut result = vec![];
    for index in 0..count {
        let a = first + index as f64 * (stitch + gap);
        let b = (a + stitch).min(last);
        if b - a > EPS {
            result.push([a, b]);
        }
    }
    if result.is_empty() {
        return Err(invalid("The pattern deposits no weld metal"));
    }
    Ok(result)
}

pub(crate) struct Built {
    pub solids: Vec<Solid>,
    pub metadata: Evaluated,
}
fn guide_points(path: &Path, offset: Vector3, start: f64, end: f64) -> Vec<[f64; 3]> {
    let divisions = if matches!(path, Path::Line { .. }) {
        1
    } else {
        64
    };
    (0..=divisions)
        .map(|index| {
            let position = start + (end - start) * index as f64 / divisions as f64;
            let point = path.point(position) + path.rotation(position) * offset;
            [point.x, point.y, point.z]
        })
        .collect()
}
pub(crate) fn record_preview(
    adapter: &MonstertruckAdapter,
    mut preview: Option<JointPreview>,
    error: Option<&CadError>,
) {
    if let Some(preview) = &mut preview {
        preview.message = error.map(ToString::to_string);
        preview.complete = error.is_none();
    }
    if let Some(preview) = preview {
        adapter.register_weld_joint(preview);
    }
}
#[allow(clippy::too_many_arguments)] // Native inputs, provenance and optional display feedback stay separate.
pub(crate) fn build(
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    resolved: &ResolvedSeam,
    explicit_supports: &[(Face, TopologyRef)],
    generated: &mut TopologyHistory,
    preview: &mut Option<JointPreview>,
) -> Result<Built> {
    let values = definition.values(doc, &owner.definition)?;
    let path = classify(&resolved.curves)?;
    *preview = Some(JointPreview {
        version: JOINT_PREVIEW_VERSION,
        owner: owner.clone(),
        source: definition.clone(),
        path: vec![],
        matrix: std::array::from_fn(|r| std::array::from_fn(|c| if r == c { 1. } else { 0. })),
        frame_label: "Owning part frame".into(),
        seam_points: guide_points(&path, Vector3::zero(), 0., path.length()),
        supports: definition.supports.clone(),
        orientation: None,
        guides: vec![],
        complete: false,
        message: None,
    });
    if definition.suppressed {
        return Ok(Built {
            solids: vec![],
            metadata: Evaluated {
                version: CONTRACT_VERSION,
                id: definition.id.clone(),
                source: owner.clone(),
                seam_length_mm: path.length(),
                deposited_length_mm: 0.,
                volume_mm3: 0.,
                section_area_mm2: 0.,
                segment_count: 0,
                segments: vec![],
                real_solid: true,
                finish: definition.finish.clone(),
                material: definition.material.clone(),
            },
        });
    }
    let mut budget = ContactBudget::new();
    let support_faces = if !explicit_supports.is_empty() {
        explicit_supports.to_vec()
    } else if !resolved.adjacent_faces.is_empty() {
        inferred_joint(definition, &values, resolved, &path, &mut budget)?
    } else {
        vec![]
    };
    if definition.supports.is_empty() {
        preview.as_mut().unwrap().supports = support_faces
            .iter()
            .map(|(_, reference)| FaceSource {
                body: definition.seam.dependency().clone(),
                reference: Box::new(reference.clone()),
                path: definition.seam.path().to_vec(),
            })
            .collect();
    }
    let supports = supports(&support_faces)?;
    if !supports.is_empty()
        && (supports.len() > 2 || (definition.profile == Profile::Fillet && supports.len() != 2))
    {
        return Err(invalid(
            "The resolved joint has the wrong number of supporting faces",
        ));
    }
    let (u, v) = frame(definition, &values, &path, &supports)?;
    preview.as_mut().unwrap().orientation = Some([[u.x, u.y, u.z], [v.x, v.y, v.z]]);
    let section = section(definition, &values, path.point(0.), path.tangent(0.), u, v)?;
    let mut sources = resolved.sources.clone();
    sources.extend(support_faces.iter().map(|(_, source)| source.clone()));
    let spans = intervals(&values, path.length())?;
    let assignments = if supports.is_empty() {
        vec![]
    } else {
        support_toes(
            definition.profile,
            &supports,
            &path,
            &section.toes,
            &spans,
            &mut budget,
        )?
    };
    for support in &supports {
        budget.charge(support, &path, &spans)?;
    }
    for (segment, [start, end]) in spans.iter().copied().enumerate() {
        for (support, assigned) in assignments.iter().enumerate() {
            for (index, (toe, offset)) in std::iter::once((0, Vector3::zero()))
                .chain(assigned.iter().copied())
                .enumerate()
            {
                preview.as_mut().unwrap().guides.push(ContactGuide {
                    label: if index == 0 {
                        "Seam root".into()
                    } else {
                        format!("Toe {}", toe + 1)
                    },
                    support,
                    segment,
                    points: guide_points(&path, offset, start, end),
                    valid: None,
                    message: None,
                });
            }
        }
    }
    let mut solids = vec![];
    let mut metadata = vec![];
    for (index, [start, end]) in spans.into_iter().enumerate() {
        for (support_index, support) in supports.iter().enumerate() {
            for (toe_index, (_, offset)) in std::iter::once((0, Vector3::zero()))
                .chain(assignments[support_index].iter().copied())
                .enumerate()
            {
                let result = if toe_index == 0 {
                    contact(support, &path, offset, start, end)
                } else {
                    footprint(support, &path, offset, start, end)
                };
                let guide = preview
                    .as_mut()
                    .unwrap()
                    .guides
                    .iter_mut()
                    .filter(|guide| guide.segment == index && guide.support == support_index)
                    .nth(toe_index)
                    .unwrap();
                guide.valid = Some(result.is_ok());
                if let Err(error) = result {
                    let message = format!(
                        "Support {} / {} / Stitch {}: {error}",
                        support_index + 1,
                        guide.label,
                        index + 1
                    );
                    guide.message = Some(message.clone());
                    return Err(match error {
                        CadError::Unsupported(_) => CadError::Unsupported(message),
                        _ => CadError::Invalid(message),
                    });
                }
            }
        }
        let transform = match &path {
            Path::Line { tangent, .. } => Matrix4::from_translation(*tangent * start),
            Path::Circle {
                center,
                axis,
                radius,
                ..
            } => {
                Matrix4::from_translation(center.to_vec())
                    * Matrix4::from_axis_angle(*axis, Rad(start / radius))
                    * Matrix4::from_translation(-center.to_vec())
            }
        };
        let face = builder::transformed(&section.face, transform);
        let wall_roles = section
            .face
            .boundaries()
            .into_iter()
            .flatten()
            .zip(face.boundaries().into_iter().flatten())
            .map(|(before, after)| (after.id(), section.wall_roles[&before.id()]))
            .collect::<HashMap<_, _>>();
        let mut history = crate::section_history::History::new(owner, "weld_bead", generated);
        let (solid, volume) = match &path {
            Path::Line { tangent, .. } => {
                let solid = crate::adapter::oriented_extrude(&face, *tangent * (end - start));
                let edges = face.boundaries().into_iter().flatten().collect::<Vec<_>>();
                let walls = solid
                    .face_iter()
                    .filter(|wall| wall.id() != face.id())
                    .collect::<Vec<_>>();
                if walls.len() != edges.len() + 1 {
                    return Err(unsupported(
                        "The weld extrusion constructor changed its native wall topology",
                    ));
                }
                history.face(
                    &face,
                    &sources,
                    serde_json::json!({"weld":definition.id,"segment":index,"cap":"start"}),
                )?;
                for (wall, edge) in walls.iter().take(edges.len()).zip(&edges) {
                    history.face(wall,&sources,serde_json::json!({"weld":definition.id,"segment":index,"profile_wall":wall_roles[&edge.id()]}))?;
                }
                history.face(
                    walls.last().unwrap(),
                    &sources,
                    serde_json::json!({"weld":definition.id,"segment":index,"cap":"finish"}),
                )?;
                (solid, section.area * (end - start))
            }
            Path::Circle {
                center,
                axis,
                radial,
                radius,
                ..
            } => {
                let hull = crate::extrusion::face_hull(&section.face)?;
                if hull.iter().any(|p| (*p - *center).dot(*radial) <= EPS) {
                    return Err(unsupported(
                        "The bead section reaches the revolution axis; reduce bead width or size",
                    ));
                }
                let angle = (end - start) / radius;
                let full = (angle - std::f64::consts::TAU).abs() <= REL;
                let divisions = (angle / std::f64::consts::FRAC_PI_2).ceil().max(2.) as usize;
                let solid: Solid = builder::revolve(
                    &face,
                    *center,
                    *axis,
                    if full {
                        builder::SweepAngle::Closed
                    } else {
                        builder::SweepAngle::Partial(Rad(angle))
                    },
                    divisions,
                );
                let edges = face.boundaries().into_iter().flatten().collect::<Vec<_>>();
                let walls = solid
                    .face_iter()
                    .filter(|wall| wall.id() != face.id())
                    .collect::<Vec<_>>();
                if walls.len() != divisions * edges.len() + usize::from(!full) {
                    return Err(unsupported(
                        "The circular weld constructor changed its native wall topology",
                    ));
                }
                for (wall_index, wall) in walls.iter().take(divisions * edges.len()).enumerate() {
                    let edge = &edges[wall_index % edges.len()];
                    history.face(wall,&sources,serde_json::json!({"weld":definition.id,"segment":index,"profile_wall":wall_roles[&edge.id()],"angular_station":wall_index/edges.len(),"angular_divisions":divisions}))?;
                }
                if !full {
                    history.face(
                        &face,
                        &sources,
                        serde_json::json!({"weld":definition.id,"segment":index,"cap":"start"}),
                    )?;
                    history.face(
                        walls.last().unwrap(),
                        &sources,
                        serde_json::json!({"weld":definition.id,"segment":index,"cap":"finish"}),
                    )?;
                }
                (
                    solid,
                    section.area * (radius + section.centroid.dot(*radial)) * angle,
                )
            }
        };
        crate::validation::solid(&solid)?;
        history.finish(&solid)?;
        if !volume.is_finite() || volume <= 0. {
            return Err(invalid("The native bead volume is not finite and positive"));
        }
        metadata.push(EvaluatedSegment {
            id: format!("{}-segment-{}", definition.id, index + 1),
            start_mm: start,
            end_mm: end,
            length_mm: end - start,
            volume_mm3: volume,
        });
        solids.push(solid);
    }
    Ok(Built {
        solids,
        metadata: Evaluated {
            version: CONTRACT_VERSION,
            id: definition.id.clone(),
            source: owner.clone(),
            seam_length_mm: path.length(),
            deposited_length_mm: metadata.iter().map(|segment| segment.length_mm).sum(),
            volume_mm3: metadata.iter().map(|segment| segment.volume_mm3).sum(),
            section_area_mm2: section.area,
            segment_count: metadata.len(),
            segments: metadata,
            real_solid: true,
            finish: definition.finish.clone(),
            material: definition.material.clone(),
        },
    })
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    if !definition.seam.path().is_empty()
        || definition.supports.iter().any(|face| !face.path.is_empty())
    {
        return Err(invalid(
            "Part welds use part-local seams; assembly welds retain their occurrence paths in the owning assembly",
        ));
    }
    let resolved = seam(adapter, doc, &definition.seam, inputs)?;
    let supports = definition
        .supports
        .iter()
        .map(|face| support(adapter, face, inputs))
        .collect::<Result<Vec<_>>>()?;
    let mut preview = None;
    let result = build(
        doc,
        owner,
        definition,
        &resolved,
        &supports,
        generated,
        &mut preview,
    );
    record_preview(adapter, preview, result.as_ref().err());
    Ok(Shape::Bodies(result?.solids))
}

pub(crate) fn metadata(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Evaluated> {
    let resolved = seam(adapter, doc, &definition.seam, inputs)?;
    let supports = definition
        .supports
        .iter()
        .map(|face| support(adapter, face, inputs))
        .collect::<Result<Vec<_>>>()?;
    let mut preview = None;
    let result = build(
        doc,
        owner,
        definition,
        &resolved,
        &supports,
        &mut TopologyHistory::default(),
        &mut preview,
    );
    record_preview(adapter, preview, result.as_ref().err());
    Ok(result?.metadata)
}
