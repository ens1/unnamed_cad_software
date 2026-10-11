//! Exact sheet stock with sewn bend interfaces and associative developed geometry.
use crate::adapter::{MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Document, NodeRef, Operation, Plane, Result, SketchCurve, sheet_metal::*,
};
use monstertruck::modeling::*;
use std::collections::BTreeMap;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Sheet metal: {}", message.to_string()))
}
fn profiles(inputs: &BTreeMap<NodeRef, Shape>, reference: &NodeRef) -> Result<Vec<Face>> {
    match inputs.get(reference) {
        Some(Shape::Profile(face)) => Ok(vec![face.clone()]),
        Some(Shape::Sketch { faces, .. }) if !faces.is_empty() => Ok(faces.clone()),
        _ => Err(invalid(
            "Select a closed source sketch; open chains cannot define sheet blanks",
        )),
    }
}
struct EdgeFlange {
    a: Point3,
    b: Point3,
    outward: Vector3,
    definition: Flange,
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
fn cross_section(
    a: Point3,
    outward: Vector3,
    normal: Vector3,
    t: f64,
    flange: &Flange,
    flat: bool,
) -> Result<Face> {
    let angle = if flat { 0. } else { flange.angle_rad };
    let length = if flat {
        flange.length_mm + flange.allowance_mm(t)
    } else {
        flange.length_mm
    };
    let center = Vector2::new(0., t * 0.5 + (flange.radius_mm + t * 0.5) * angle.signum());
    let rotate = |point: Vector2, theta: f64| {
        let p = point - center;
        center
            + Vector2::new(
                p.x * theta.cos() - p.y * theta.sin(),
                p.x * theta.sin() + p.y * theta.cos(),
            )
    };
    let top = Vector2::new(0., t);
    let bottom = Vector2::new(0., 0.);
    let top_bend = rotate(top, angle);
    let bottom_bend = rotate(bottom, angle);
    let extension = Vector2::new(angle.cos(), angle.sin()) * length;
    let points = if flat {
        vec![top, top + extension, bottom + extension, bottom]
    } else {
        vec![
            top,
            top_bend,
            top_bend + extension,
            bottom_bend + extension,
            bottom_bend,
            bottom,
        ]
    };
    let vertices = points
        .iter()
        .map(|p| builder::vertex(a + outward * p.x + normal * p.y))
        .collect::<Vec<_>>();
    let mut edges = vec![];
    for index in 0..vertices.len() {
        let front = &vertices[index];
        let back = &vertices[(index + 1) % vertices.len()];
        let transit = if !flat && index == 0 {
            Some(rotate(top, angle * 0.5))
        } else if !flat && index == 4 {
            Some(rotate(bottom, angle * 0.5))
        } else {
            None
        };
        let edge: Edge = if let Some(p) = transit {
            builder::try_circle_arc(front, back, a + outward * p.x + normal * p.y)
                .map_err(invalid)?
        } else {
            builder::line(front, back)
        };
        edges.push(edge);
    }
    builder::try_attach_plane(vec![edges.into()]).map_err(invalid)
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
fn join(base: Solid, extra: Solid, flange: &EdgeFlange, normal: Vector3, t: f64) -> Result<Solid> {
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
    let result = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&result)?;
    Ok(result)
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
    stock: &Stock,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
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
    let base_faces = profiles(inputs, &stock.profile)?;
    let mut plates = base_faces
        .iter()
        .map(|face| {
            crate::adapter::oriented_extrude(
                &builder::translated(face, shift),
                normal * stock.thickness_mm,
            )
        })
        .collect::<Vec<_>>();
    for tab in &stock.tabs {
        if plates.len() != 1 {
            return Err(CadError::Unsupported(
                "Sheet tabs currently require one connected base blank".into(),
            ));
        }
        let (_, tab_plane) = crate::extrusion::source(adapter, doc, tab, inputs)?;
        let tab_n = Vector3::from(tab_plane.u)
            .cross(Vector3::from(tab_plane.v))
            .normalize();
        if tab_n.cross(normal).magnitude() > 1e-8
            || (Point3::from(tab_plane.origin) - Point3::from(plane.origin))
                .dot(normal)
                .abs()
                > 1e-7
        {
            return Err(invalid("Tab sketch must be coplanar with its base sketch"));
        }
        for face in profiles(inputs, tab)? {
            let tab = crate::adapter::oriented_extrude(
                &builder::translated(&face, shift),
                normal * stock.thickness_mm,
            );
            plates[0] = monstertruck::solid::or(&plates[0], &tab, tolerance).map_err(invalid)?;
            crate::validation::solid(&plates[0])?;
        }
    }
    if stock.flanges.is_empty() {
        return Ok(Built {
            flat: plates.clone(),
            solids: plates,
            plane,
            bends: vec![],
        });
    }
    if base_faces.len() != 1 || plates.len() != 1 {
        return Err(CadError::Unsupported("Edge flanges currently require one connected base region; separate sheet blanks need separate features".into()));
    }
    let face = if base_faces[0].oriented_surface().normal(0., 0.).dot(normal) > 0. {
        base_faces[0].clone()
    } else {
        base_faces[0].inverse()
    };
    let outer = face
        .boundaries()
        .first()
        .cloned()
        .ok_or_else(|| invalid("Base sketch has no outer contour"))?;
    let bounds = face_bounds(&face)?;
    let mut edges = vec![];
    let mut bends = vec![];
    for definition in &stock.flanges {
        if !sketch.members.contains_key(&definition.member) {
            return Err(invalid("Selected flange sketch member was removed"));
        }
        let member = cad_core::sketch_edit::effective_curve(&sketch, &definition.member)?;
        let SketchCurve::Line { start, end } = member else {
            return Err(CadError::Unsupported("Edge flanges currently require a straight line sketch member on the outer boundary".into()));
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
                "Flange member is missing, ambiguous, construction geometry, or not one complete outer edge",
            ));
        }
        let a = candidates[0].front().point();
        let b = candidates[0].back().point();
        let outward = (b - a).normalize().cross(normal).normalize();
        if bounds.iter().any(|p| (*p - a).dot(outward) > 1e-7) {
            return Err(CadError::Unsupported("Edge flange needs a supporting outer edge; concave or curved stock extends beyond its bend plane".into()));
        }
        let formed = (definition.radius_mm + stock.thickness_mm) * definition.angle_rad.abs().sin()
            + definition.length_mm * definition.angle_rad.cos();
        let flat = definition.length_mm + definition.allowance_mm(stock.thickness_mm);
        let edge = EdgeFlange {
            a: a + shift,
            b: b + shift,
            outward,
            definition: definition.clone(),
            envelope: formed.max(flat),
        };
        if edges
            .iter()
            .any(|other| envelope_overlap(&edge, other, normal))
        {
            return Err(CadError::Unsupported("Selected flange development envelopes overlap; add corner relief or shorten flanges before forming".into()));
        }
        let center = edge.a + edge.outward * (definition.allowance_mm(stock.thickness_mm) * 0.5)
            - Point3::from(plane.origin);
        let delta = edge.b - edge.a;
        let project = |p: Vector3| [p.dot(Vector3::from(plane.u)), p.dot(Vector3::from(plane.v))];
        bends.push(BendLine {
            feature: definition.source.clone(),
            member: definition.member.clone(),
            start: project(center),
            end: project(center + delta),
            angle_rad: definition.angle_rad,
            inside_radius_mm: definition.radius_mm,
            allowance_mm: definition.allowance_mm(stock.thickness_mm),
            k_factor: definition.k_factor,
        });
        edges.push(edge);
    }
    let mut formed = plates.remove(0);
    let mut flat = formed.clone();
    for flange in edges {
        let flat_face = cross_section(
            flange.a,
            flange.outward,
            normal,
            stock.thickness_mm,
            &flange.definition,
            true,
        )?;
        let formed_face = cross_section(
            flange.a,
            flange.outward,
            normal,
            stock.thickness_mm,
            &flange.definition,
            false,
        )?;
        flat = join(
            flat,
            crate::adapter::oriented_extrude(&flat_face, flange.b - flange.a),
            &flange,
            normal,
            stock.thickness_mm,
        )?;
        formed = join(
            formed,
            crate::adapter::oriented_extrude(&formed_face, flange.b - flange.a),
            &flange,
            normal,
            stock.thickness_mm,
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
) -> Result<Shape> {
    let stock = stock(doc, owner)?;
    let built = build(adapter, doc, &stock, inputs, tolerance)?;
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
    let built = build(adapter, doc, &stock, inputs, tolerance)?;
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
