//! Exact sheet stock with sewn bend interfaces and associative developed geometry.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Document, NodeRef, Operation, Plane, Result, SketchCurve, TopologyRef, sheet_metal::*,
};
use monstertruck::modeling::*;
use std::collections::BTreeMap;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Sheet metal: {}", message.to_string()))
}
struct EdgeFlange {
    a: Point3,
    b: Point3,
    outward: Vector3,
    definitions: Vec<Flange>,
    envelope: f64,
}
struct Built {
    solids: Vec<Solid>,
    flat: Vec<Solid>,
    plane: Plane,
    bends: Vec<BendLine>,
}
fn face_bounds(face: &Face) -> Result<Vec<Point3>> {
    let mut points = vec![];
    for edge in face.edge_iter() {
        let curve=crate::adapter::exact_curve(&edge.curve()).ok_or_else(||CadError::Unsupported("Sheet flange supporting-plane checks require exact positive-weight rational base curves".into()))?;
        points.extend(curve.control_points.into_iter().map(Point3::from));
    }
    Ok(points)
}
fn rotation(p: Vector2, angle: f64) -> Vector2 {
    let (s, c) = angle.sin_cos();
    Vector2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}
/// Every bend remains an exact circular carrier; straight tangents share vertices.
/// Whole connected flange chains are swept as one region so intermediate joints
/// cannot leave duplicate coincident walls or change developed stock length.
fn cross_section(
    a: Point3,
    outward: Vector3,
    normal: Vector3,
    t: f64,
    flanges: &[Flange],
    flat: bool,
) -> Result<Face> {
    let mut bottom = Vector2::zero();
    let mut direction = Vector2::unit_x();
    let mut up = Vector2::unit_y();
    let mut upper = vec![];
    let mut lower = vec![];
    let mut bottom_vertex = builder::vertex(a);
    let start_bottom = bottom_vertex.clone();
    let mut top_vertex = builder::vertex(a + normal * t);
    let start_top = top_vertex.clone();
    let lift = |p: Vector2| a + outward * p.x + normal * p.y;
    for bend in flanges.iter().flat_map(Flange::segments) {
        if flat {
            let extension = direction * (bend.length_mm + bend.allowance_mm(t));
            let next_bottom = builder::vertex(lift(bottom + extension));
            let next_top = builder::vertex(lift(bottom + up * t + extension));
            lower.push(builder::line(&bottom_vertex, &next_bottom));
            upper.push(builder::line(&top_vertex, &next_top));
            bottom += extension;
            bottom_vertex = next_bottom;
            top_vertex = next_top;
            continue;
        }
        let top = bottom + up * t;
        let center = bottom + up * (t * 0.5 + (bend.radius_mm + t * 0.5) * bend.angle_rad.signum());
        let at = |p: Vector2, angle: f64| center + rotation(p - center, angle);
        let bent_bottom = at(bottom, bend.angle_rad);
        let bent_top = at(top, bend.angle_rad);
        let next_bottom = builder::vertex(lift(bent_bottom));
        let next_top = builder::vertex(lift(bent_top));
        lower.push(
            builder::try_circle_arc(
                &bottom_vertex,
                &next_bottom,
                lift(at(bottom, bend.angle_rad * 0.5)),
            )
            .map_err(invalid)?,
        );
        upper.push(
            builder::try_circle_arc(&top_vertex, &next_top, lift(at(top, bend.angle_rad * 0.5)))
                .map_err(invalid)?,
        );
        direction = rotation(direction, bend.angle_rad);
        up = rotation(up, bend.angle_rad);
        bottom = bent_bottom + direction * bend.length_mm;
        let end_bottom = builder::vertex(lift(bottom));
        let end_top = builder::vertex(lift(bottom + up * t));
        lower.push(builder::line(&next_bottom, &end_bottom));
        upper.push(builder::line(&next_top, &end_top));
        bottom_vertex = end_bottom;
        top_vertex = end_top;
    }
    let mut edges = upper;
    edges.push(builder::line(&top_vertex, &bottom_vertex));
    edges.extend(lower.into_iter().rev().map(|edge| edge.inverse()));
    edges.push(builder::line(&start_bottom, &start_top));
    let face = builder::try_attach_plane(vec![edges.into()]).map_err(invalid)?;
    if !flat {
        validate_section(&face, a, outward, normal, t)?;
    }
    Ok(face)
}
fn validate_section(
    face: &Face,
    a: Point3,
    outward: Vector3,
    normal: Vector3,
    thickness: f64,
) -> Result<()> {
    use crate::join::region::{Frame, Location, checked_intersections, locate};
    let frame = Frame {
        origin: a,
        axes: [outward, normal],
        normal: outward.cross(normal),
    };
    let edges = face.boundaries()[0].iter().cloned().collect::<Vec<_>>();
    if edges.len() > 4096 {
        return Err(CadError::Unsupported(
            "Sheet flange chain exceeds its bounded exact section work".into(),
        ));
    }
    let segments = edges
        .iter()
        .map(|edge| {
            frame
                .segment(edge)?
                .ok_or_else(|| invalid("Flange section contains a non-analytic line/arc boundary"))
        })
        .collect::<Result<Vec<_>>>()?;
    let hulls = edges
        .iter()
        .map(|edge| {
            crate::extrusion::curve_hull(&edge.oriented_curve()).map(|points| {
                points
                    .into_iter()
                    .map(|point| frame.project(point))
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let range = |points: &[Point2], axis: usize| {
        (
            points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min),
            points
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max),
        )
    };
    for (i, left) in segments.iter().enumerate() {
        for (j, right) in segments.iter().enumerate().skip(i + 1) {
            let adjacent = j == i + 1 || i == 0 && j + 1 == segments.len();
            // Adjacent pieces are generated with a shared vertex and a known
            // tangent/radial construction. Re-recognizing their circles and
            // certifying an independently rounded endpoint would reject the
            // authored tangent itself. Their common endpoint is proven by
            // construction, and these bounded arcs cannot meet the adjacent
            // outward tangent or radial end again.
            if adjacent {
                continue;
            }
            if (0..2).any(|axis| {
                let a = range(&hulls[i], axis);
                let b = range(&hulls[j], axis);
                a.1 < b.0 - 1e-7 || b.1 < a.0 - 1e-7
            }) {
                continue;
            }
            if !checked_intersections(left, right)?.is_empty() {
                return Err(invalid(
                    "Formed flange chain intersects or touches itself; adjust tangent lengths, bend directions or radii",
                ));
            }
        }
    }
    // The selected supporting base edge bounds all original blank material on
    // x<=0. Admit its shared interface and reject all additional contact with
    // that half-strip, including returned flanges colliding with the base.
    let low = face_bounds(face)?
        .iter()
        .map(|p| (*p - a).dot(outward))
        .fold(0., f64::min)
        - 1.;
    let corners = [
        Vector2::new(0., 0.),
        Vector2::new(0., thickness),
        Vector2::new(low, thickness),
        Vector2::new(low, 0.),
    ]
    .map(|p| builder::vertex(a + outward * p.x + normal * p.y));
    let base_edges = (0..4)
        .map(|i| builder::line(&corners[i], &corners[(i + 1) % 4]))
        .collect::<Vec<_>>();
    let base = base_edges
        .iter()
        .map(|edge| {
            frame
                .segment(edge)?
                .ok_or_else(|| invalid("Invalid supporting section"))
        })
        .collect::<Result<Vec<_>>>()?;
    for (index, section) in segments.iter().enumerate().take(segments.len() - 1) {
        let x = range(&hulls[index], 0);
        let y = range(&hulls[index], 1);
        if x.0 >= -1e-7 || y.0 > thickness + 1e-7 || y.1 < -1e-7 {
            continue;
        }
        for edge in &base {
            for point in checked_intersections(section, edge)? {
                if !(point.distance(Point2::new(0., 0.)) <= 1e-7
                    || point.distance(Point2::new(0., thickness)) <= 1e-7)
                {
                    return Err(invalid(
                        "Returned flange collides with the supporting blank; increase clearance or change bend direction",
                    ));
                }
            }
        }
        if locate(section.at(0.5), &base) == Location::Inside {
            return Err(invalid("Returned flange penetrates the supporting blank"));
        }
    }
    if locate(Point2::new(-1e-5, thickness * 0.5), &segments) == Location::Inside {
        return Err(invalid("Formed flange encloses supporting blank material"));
    }
    Ok(())
}
fn interface(face: &Face, a: Point3, b: Point3, n: Vector3, t: f64, epsilon: f64) -> bool {
    let targets = [a, b, a + n * t, b + n * t];
    let vertices = face.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
    vertices.len() == 4
        && vertices
            .iter()
            .all(|p| targets.iter().any(|q| (*p - *q).magnitude() <= epsilon))
        && targets
            .iter()
            .all(|q| vertices.iter().any(|p| (*p - *q).magnitude() <= epsilon))
}
fn join(
    base: Solid,
    extra: Solid,
    flange: &EdgeFlange,
    normal: Vector3,
    t: f64,
    history: &mut TopologyHistory,
) -> Result<Solid> {
    let epsilon = (t * 1e-5).min(1e-7);
    let mut removed = [0; 2];
    let mut faces = vec![];
    for (index, solid) in [&base, &extra].into_iter().enumerate() {
        for face in solid.face_iter() {
            if interface(face, flange.a, flange.b, normal, t, epsilon) {
                removed[index] += 1;
            } else {
                faces.push(face.clone());
            }
        }
    }
    if removed != [1, 1] {
        return Err(CadError::Unsupported("Edge flange needs one complete unsplit straight base sidewall; tabs or other operations changed the selected member boundary".into()));
    }
    let shell = crate::surfaces::knit(&faces, epsilon)?;
    for (before, after) in faces.iter().zip(shell.iter()) {
        if let Some(source) = history.face_source(before.id()) {
            history.inherit_face(after.id(), source);
        }
        for edge in after.edge_iter() {
            let candidates = before
                .edge_iter()
                .filter(|old| {
                    crate::surfaces::same_curve(
                        &old.oriented_curve(),
                        &edge.oriented_curve(),
                        epsilon,
                    ) || crate::surfaces::same_curve(
                        &old.inverse().oriented_curve(),
                        &edge.oriented_curve(),
                        epsilon,
                    )
                })
                .filter_map(|old| history.edge_source(old.id()))
                .collect::<Vec<_>>();
            if let [source] = candidates.as_slice() {
                history.inherit_edge(edge.id(), source.clone());
            }
        }
    }
    let result = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&result)?;
    Ok(result)
}
fn source_reference(source: &NodeRef, member: Option<&str>, topology: &str) -> TopologyRef {
    TopologyRef {
        producer: source.clone(),
        role: member
            .map(|id| format!("member:{id}"))
            .unwrap_or_else(|| "profile".into()),
        topology: topology.into(),
        signature: None,
    }
}
/// Record stock construction before native partition Booleans consume it.
/// Sources are definition/member identities, not tessellation or face indices.
fn record_prism(
    owner: &NodeRef,
    source: &NodeRef,
    section: &crate::extrusion::ThinSection,
    direction: Vector3,
    solid: &Solid,
    purpose: &serde_json::Value,
    history: &mut TopologyHistory,
) -> Result<()> {
    // Native Sketch inputs do not publish a solid carrier history. Rebuilding
    // a traced section obtains each authored member and constructor piece
    // directly; a whole-profile fallback would name every sidewall alike.
    // The shared extrusion recorder also names cap/wall intersections before
    // a tab or normal cut asks a Boolean to propagate their ancestry.
    crate::section_history::extrusion(
        owner,
        source,
        section,
        solid,
        crate::section_history::ToolContext {
            axis: direction.normalize(),
            drafted: false,
        },
        purpose,
        history,
    )
}
fn record_strip(
    owner: &NodeRef,
    section: &Face,
    strip: &EdgeFlange,
    solid: &Solid,
    flat: bool,
    history: &mut TopologyHistory,
) -> Result<()> {
    let mut upper = vec![];
    let mut lower = vec![];
    for feature in &strip.definitions {
        for (index, _) in feature.segments().enumerate() {
            let source = source_reference(&feature.source, Some(&feature.member), "edge");
            if !flat {
                upper.push((source.clone(), serde_json::json!({"operation":"sheet_flange","bend":index,"surface":"upper_bend"})));
                lower.push((source.clone(), serde_json::json!({"operation":"sheet_flange","bend":index,"surface":"lower_bend"})));
            }
            upper.push((source.clone(), serde_json::json!({"operation":"sheet_flange","bend":index,"surface":if flat {"upper_developed"} else {"upper_tangent"}})));
            lower.push((source, serde_json::json!({"operation":"sheet_flange","bend":index,"surface":if flat {"lower_developed"} else {"lower_tangent"}})));
        }
    }
    let first = strip
        .definitions
        .first()
        .ok_or_else(|| invalid("Flange strip has no definition"))?;
    let last = strip
        .definitions
        .last()
        .ok_or_else(|| invalid("Flange strip has no definition"))?;
    let mut tags = upper;
    tags.push((
        source_reference(&last.source, Some(&last.member), "edge"),
        serde_json::json!({"operation":"sheet_flange","surface":"terminal"}),
    ));
    tags.extend(lower.into_iter().rev());
    tags.push((
        source_reference(&first.source, Some(&first.member), "edge"),
        serde_json::json!({"operation":"sheet_flange","surface":"attachment"}),
    ));
    let edges = section.boundaries()[0].iter().cloned().collect::<Vec<_>>();
    if tags.len() != edges.len() {
        return Err(invalid(
            "Flange section ancestry does not match its native boundary",
        ));
    }
    let axis = (strip.b - strip.a).normalize();
    for face in solid.face_iter() {
        let normal = face.oriented_surface().normal(0., 0.).normalize();
        if normal.dot(axis).abs() > 1. - 1e-8 {
            let sources = strip
                .definitions
                .iter()
                .map(|feature| source_reference(&feature.source, Some(&feature.member), "edge"))
                .collect::<Vec<_>>();
            history.constructed_face(owner, face.id(), &sources, &serde_json::json!({"operation":"sheet_flange_width_cap","end":if normal.dot(axis)>0. {1} else {0},"state":if flat {"flat"} else {"formed"}}))?;
        } else {
            let found = edges
                .iter()
                .enumerate()
                .find(|(_, edge)| {
                    face.edge_iter().any(|candidate| {
                        crate::surfaces::same_curve(
                            &candidate.oriented_curve(),
                            &edge.oriented_curve(),
                            1e-7,
                        ) || crate::surfaces::same_curve(
                            &candidate.oriented_curve(),
                            &edge.inverse().oriented_curve(),
                            1e-7,
                        )
                    })
                })
                .ok_or_else(|| invalid("Flange wall lost its exact section boundary ancestry"))?;
            let (source, purpose) = &tags[found.0];
            history.constructed_face(owner, face.id(), std::slice::from_ref(source), purpose)?;
        }
    }
    Ok(())
}
fn envelope_overlap(a: &EdgeFlange, b: &EdgeFlange, normal: Vector3) -> bool {
    let polygon = |f: &EdgeFlange| {
        [
            f.a,
            f.b,
            f.b + f.outward * f.envelope,
            f.a + f.outward * f.envelope,
        ]
    };
    let aa = polygon(a);
    let bb = polygon(b);
    for axis in [
        a.outward,
        (a.b - a.a).normalize(),
        b.outward,
        (b.b - b.a).normalize(),
        normal,
    ] {
        if axis.dot(normal).abs() > 0.9 {
            continue;
        }
        let range = |points: &[Point3; 4]| {
            let values = points.map(|p| p.to_vec().dot(axis));
            [
                values.iter().copied().fold(f64::INFINITY, f64::min),
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        let x = range(&aa);
        let y = range(&bb);
        if x[1] <= y[0] + 1e-7 || y[1] <= x[0] + 1e-7 {
            return false;
        }
    }
    true
}
fn build(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    stock: &Stock,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    history: &mut TopologyHistory,
) -> Result<Built> {
    let (sketch, plane) = crate::extrusion::source(adapter, doc, &stock.profile, inputs)?;
    let normal = Vector3::from(plane.u)
        .cross(Vector3::from(plane.v))
        .normalize()
        * if stock.side == ThicknessSide::Negative {
            -1.
        } else {
            1.
        };
    let shift = if stock.side == ThicknessSide::MidPlane {
        -normal * stock.thickness_mm * 0.5
    } else {
        Vector3::zero()
    };
    let base_sections =
        crate::extrusion::profiles_traced(adapter, doc, &stock.profile, inputs, &[])?;
    let mut plates = vec![];
    for section in &base_sections {
        let plate = crate::adapter::oriented_extrude(
            &builder::translated(&section.face, shift),
            normal * stock.thickness_mm,
        );
        record_prism(
            &stock.source,
            &stock.profile,
            section,
            normal * stock.thickness_mm,
            &plate,
            &serde_json::json!({"operation":"sheet_stock_extrusion","source":stock.source}),
            history,
        )?;
        plates.push(plate);
    }
    for edit in &stock.blank_edits {
        let (source, profile, mode) = match edit {
            BlankEdit::Tab { source, profile } => {
                (source, profile, crate::boolean_ops::Mode::Union)
            }
            BlankEdit::Cut { source, profile } => {
                (source, profile, crate::boolean_ops::Mode::Difference)
            }
        };
        let (_, edit_plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
        let edit_n = Vector3::from(edit_plane.u)
            .cross(Vector3::from(edit_plane.v))
            .normalize();
        let plane_offset =
            (Point3::from(edit_plane.origin) - Point3::from(plane.origin)).dot(normal);
        if edit_n.cross(normal).magnitude() > 1e-8
            || matches!(mode, crate::boolean_ops::Mode::Union) && plane_offset.abs() > 1e-7
        {
            return Err(invalid(
                "Sheet tabs must be coplanar with the original blank; normal-cut sketches may lie on any parallel plane, including its top face",
            ));
        }
        let edit_shift = shift
            - if matches!(mode, crate::boolean_ops::Mode::Difference) {
                normal * plane_offset
            } else {
                Vector3::zero()
            };
        for section in crate::extrusion::profiles_traced(adapter, doc, profile, inputs, &[])? {
            let tool = crate::adapter::oriented_extrude(
                &builder::translated(&section.face, edit_shift),
                normal * stock.thickness_mm,
            );
            record_prism(
                source,
                profile,
                &section,
                normal * stock.thickness_mm,
                &tool,
                &serde_json::json!({"operation":"sheet_blank_edit","source":source}),
                history,
            )?;
            let scope = std::collections::BTreeSet::from([
                stock.source.clone(),
                stock.profile.clone(),
                source.clone(),
                profile.clone(),
            ]);
            if matches!(mode, crate::boolean_ops::Mode::Union) {
                if plates.len() != 1 {
                    return Err(CadError::Unsupported("Sheet tabs need one connected base region; separate blanks retain separate stock definitions".into()));
                }
                if crate::boolean_ops::disjoint(&plates[0], &tool) {
                    return Err(invalid(
                        "Tab must overlap its base blank; detached tab regions require another base feature",
                    ));
                }
                plates[0] = crate::boolean_ops::union_with_history(
                    &plates[0],
                    &tool,
                    tolerance,
                    &scope,
                    crate::revolution::Combination {
                        adapter,
                        owner,
                        generated: history,
                    },
                )?;
            } else {
                let mut remaining = vec![];
                for plate in plates {
                    if let Some(plate) = crate::boolean_ops::binary_with_history(
                        &plate,
                        &tool,
                        mode,
                        tolerance,
                        &scope,
                        crate::revolution::Combination {
                            adapter,
                            owner,
                            generated: history,
                        },
                    )? {
                        remaining.push(plate);
                    }
                }
                plates = remaining;
            }
        }
    }
    if plates.is_empty() {
        return Err(invalid("Sheet normal cut removed the complete blank"));
    }
    if stock.flanges.is_empty() {
        return Ok(Built {
            flat: plates.clone(),
            solids: plates,
            plane,
            bends: vec![],
        });
    }
    if base_sections.len() != 1 || plates.len() != 1 || plates[0].boundaries().len() != 1 {
        return Err(CadError::Unsupported("Flanges require one connected blank; separate sheet regions need separate stock features".into()));
    }
    let face = if base_sections[0]
        .face
        .oriented_surface()
        .normal(0., 0.)
        .dot(normal)
        > 0.
    {
        base_sections[0].face.clone()
    } else {
        base_sections[0].face.inverse()
    };
    let outer = face
        .boundaries()
        .first()
        .cloned()
        .ok_or_else(|| invalid("Base sketch has no outer contour"))?;
    let bounds = face_bounds(&face)?;
    let mut strips = vec![];
    let mut bends = vec![];
    for root in stock
        .flanges
        .iter()
        .filter(|feature| feature.attachment.is_none())
    {
        if !sketch.members.contains_key(&root.member) {
            return Err(invalid("Selected flange sketch member was removed"));
        }
        let member = cad_core::sketch_edit::effective_curve(&sketch, &root.member)?;
        let SketchCurve::Line { start, end } = member else {
            return Err(CadError::Unsupported(
                "Flanges require a complete straight outer base-sketch member".into(),
            ));
        };
        let at = |key: &str| {
            let p = sketch
                .points
                .get(key)
                .ok_or_else(|| invalid("Flange endpoint was removed"))?;
            Ok::<_, CadError>(
                Point3::from(plane.origin)
                    + Vector3::from(plane.u) * p[0]
                    + Vector3::from(plane.v) * p[1],
            )
        };
        let pa = at(&start)?;
        let pb = at(&end)?;
        let candidates = outer
            .iter()
            .filter(|edge| {
                matches!(edge.curve(), Curve::Line(_))
                    && ((edge.front().point() - pa).magnitude() < 1e-7
                        && (edge.back().point() - pb).magnitude() < 1e-7
                        || (edge.front().point() - pb).magnitude() < 1e-7
                            && (edge.back().point() - pa).magnitude() < 1e-7)
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(invalid(
                "Flange member is missing, construction, ambiguous, or not one complete outer edge",
            ));
        }
        let a = candidates[0].front().point();
        let b = candidates[0].back().point();
        let outward = (b - a).normalize().cross(normal).normalize();
        if bounds.iter().any(|p| (*p - a).dot(outward) > 1e-7) {
            return Err(CadError::Unsupported("Base flange attachment needs a supporting outer edge; concave or curved stock extends beyond its bend plane".into()));
        }
        let mut definitions = vec![root.clone()];
        loop {
            let current = &definitions
                .last()
                .ok_or_else(|| invalid("Missing chain root"))?
                .source;
            let Some(next) = stock
                .flanges
                .iter()
                .find(|feature| feature.attachment.as_ref() == Some(current))
            else {
                break;
            };
            definitions.push(next.clone());
            if definitions.len() > stock.flanges.len() {
                return Err(invalid("Flange chain has a cyclic attachment"));
            }
        }
        let mut developed = 0.;
        for feature in &definitions {
            for (index, bend) in feature.segments().enumerate() {
                let allowance = bend.allowance_mm(stock.thickness_mm);
                let center =
                    a + outward * (developed + allowance * 0.5) - Point3::from(plane.origin);
                let delta = b - a;
                let project =
                    |p: Vector3| [p.dot(Vector3::from(plane.u)), p.dot(Vector3::from(plane.v))];
                bends.push(BendLine {
                    feature: feature.source.clone(),
                    member: feature.member.clone(),
                    index: index as u32,
                    start: project(center),
                    end: project(center + delta),
                    angle_rad: bend.angle_rad,
                    inside_radius_mm: bend.radius_mm,
                    allowance_mm: allowance,
                    k_factor: bend.k_factor,
                });
                developed += allowance + bend.length_mm;
            }
        }
        if !developed.is_finite() || developed > 1e9 {
            return Err(invalid(
                "Developed flange chain exceeds the finite manufacturing span",
            ));
        }
        let strip = EdgeFlange {
            a: a + shift,
            b: b + shift,
            outward,
            definitions,
            envelope: developed,
        };
        if strips
            .iter()
            .any(|other| envelope_overlap(&strip, other, normal))
        {
            return Err(CadError::Unsupported("Selected flange flat-pattern envelopes overlap; shorten flanges or use non-overlapping blanks".into()));
        }
        strips.push(strip);
    }
    let mut formed = plates.remove(0);
    let mut flat = formed.clone();
    let mut formed_strips: Vec<(Solid, Vector3)> = vec![];
    for flange in strips {
        let flat_face = cross_section(
            flange.a,
            flange.outward,
            normal,
            stock.thickness_mm,
            &flange.definitions,
            true,
        )?;
        let formed_face = cross_section(
            flange.a,
            flange.outward,
            normal,
            stock.thickness_mm,
            &flange.definitions,
            false,
        )?;
        let flat_strip = crate::adapter::oriented_extrude(&flat_face, flange.b - flange.a);
        let formed_strip = crate::adapter::oriented_extrude(&formed_face, flange.b - flange.a);
        // Exact control enclosures are sufficient to certify separation. When
        // returned chains overlap their enclosures, do not silently create an
        // intersecting non-manifold sheet or substitute mesh stitching.
        for (prior, prior_outward) in &formed_strips {
            if !crate::boolean_ops::disjoint(prior, &formed_strip) {
                let prior_points = crate::adapter::solid_control_points(prior)?;
                let next_points = crate::adapter::solid_control_points(&formed_strip)?;
                let axes = [
                    normal,
                    *prior_outward,
                    prior_outward.cross(normal),
                    flange.outward,
                    flange.outward.cross(normal),
                ];
                let separated = axes.into_iter().any(|axis| {
                    let range = |points: &[Point3]| {
                        let mut low = f64::INFINITY;
                        let mut high = f64::NEG_INFINITY;
                        for point in points {
                            let value = point.to_vec().dot(axis);
                            low = low.min(value);
                            high = high.max(value);
                        }
                        (low, high)
                    };
                    let a = range(&prior_points);
                    let b = range(&next_points);
                    a.1 <= b.0 + 1e-7 || b.1 <= a.0 + 1e-7
                });
                if !separated {
                    return Err(CadError::Unsupported("Formed flange chains have overlapping carrier enclosures; change return lengths or bend directions to provide proven clearance".into()));
                }
            }
        }
        record_strip(owner, &flat_face, &flange, &flat_strip, true, history)?;
        record_strip(owner, &formed_face, &flange, &formed_strip, false, history)?;
        formed_strips.push((formed_strip.clone(), flange.outward));
        flat = join(
            flat,
            flat_strip,
            &flange,
            normal,
            stock.thickness_mm,
            history,
        )?;
        formed = join(
            formed,
            formed_strip,
            &flange,
            normal,
            stock.thickness_mm,
            history,
        )?;
    }
    Ok(Built {
        solids: vec![formed],
        flat: vec![flat],
        plane,
        bends,
    })
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    history: &mut TopologyHistory,
) -> Result<Shape> {
    let stock = stock(doc, owner)?;
    let built = build(adapter, doc, owner, &stock, inputs, tolerance, history)?;
    Ok(Shape::Bodies(if stock.state == State::Flat {
        built.flat
    } else {
        built.solids
    }))
}
pub(crate) fn metadata(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
) -> Result<Option<EvaluatedSheet>> {
    let operation = doc
        .parts
        .get(&owner.definition)
        .and_then(|part| part.features.iter().find(|f| f.id == owner.node))
        .map(|f| &f.operation);
    if !matches!(operation, Some(Operation::Sheet { .. })) {
        return Ok(None);
    }
    let stock = stock(doc, owner)?;
    let built = build(
        adapter,
        doc,
        owner,
        &stock,
        inputs,
        tolerance,
        &mut TopologyHistory::default(),
    )?;
    let normal = Vector3::from(built.plane.u)
        .cross(Vector3::from(built.plane.v))
        .normalize()
        * if stock.side == ThicknessSide::Negative {
            -1.
        } else {
            1.
        };
    let mut contours = vec![];
    let mut export_error = None;
    for solid in &built.flat {
        let top = solid
            .face_iter()
            .filter(|face| {
                face.oriented_surface()
                    .normal(0., 0.)
                    .normalize()
                    .dot(normal)
                    > 1. - 1e-8
            })
            .collect::<Vec<_>>();
        let mut counts = std::collections::HashMap::new();
        for face in &top {
            for edge in face.edge_iter() {
                *counts.entry(edge.id()).or_insert(0) += 1;
            }
        }
        for face in top {
            for edge in face.edge_iter().filter(|edge| counts[&edge.id()] == 1) {
                if let Some(curve) = crate::adapter::exact_curve(&edge.oriented_curve()) {
                    contours.push(cad_core::sketch_edit::project(&curve, &built.plane)?);
                } else {
                    export_error=Some("Flat-pattern DXF cannot encode an exact outline intersection curve; unsupported curves are not replaced with sampled cutting paths".into());
                }
            }
        }
    }
    Ok(Some(EvaluatedSheet {
        version: CONTRACT_VERSION,
        source: owner.clone(),
        base_source: stock.source.clone(),
        profile: stock.profile.clone(),
        state: stock.state,
        plane: built.plane,
        thickness_mm: stock.thickness_mm,
        bends: built.bends,
        flat_contours: contours,
        flat_export_error: export_error,
    }))
}
