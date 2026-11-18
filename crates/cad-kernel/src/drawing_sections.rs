//! Disposable native drawing sections. Geometry and reference intersections
//! come from the same current model configuration and occurrence placement.
use cad_core::{
    CadError, Document, Drawing, Result,
    drawing::{self, CurveReference},
    drawing_sections::{self as section, Definition, Evaluated, Frame, Region, Vertex},
    protocol::Evaluation,
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap, HashSet};
fn unsupported(s: &str) -> CadError {
    CadError::Unsupported(format!("Drawing section: {s}"))
}
type Point = [f64; 3];
fn at(p: Point, m: [[f64; 4]; 4]) -> Point {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}
/// Cancel shared boundaries between coplanar cut patches before outlining and
/// hatching. A counterbore or partial-depth pocket can divide the cap across
/// several axial layers even though its material is continuous on the paper.
fn perimeter(caps: Vec<Face>) -> Result<Vec<Vec<Edge>>> {
    let mut uses: HashMap<EdgeId, Vec<Edge>> = HashMap::new();
    let mut order = Vec::new();
    for cap in caps {
        for edge in cap.boundaries().into_iter().flatten() {
            if !uses.contains_key(&edge.id()) {
                order.push(edge.id());
            }
            uses.entry(edge.id()).or_default().push(edge);
        }
    }
    let mut outgoing = HashMap::new();
    let mut starts = Vec::new();
    for id in order {
        let edges = uses.remove(&id).unwrap();
        match edges.as_slice() {
            [edge] => {
                starts.push(edge.front().id());
                if outgoing.insert(edge.front().id(), edge.clone()).is_some() {
                    return Err(unsupported("cap regions touch at an ambiguous vertex"));
                }
            }
            [a, b] if a.orientation() != b.orientation() => {}
            _ => {
                return Err(unsupported(
                    "the cut faces have overlapping or non-manifold boundaries",
                ));
            }
        }
    }
    let mut loops = Vec::new();
    let mut visited = HashSet::new();
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut cursor = start;
        let mut wire = Vec::new();
        loop {
            let edge = outgoing
                .remove(&cursor)
                .ok_or_else(|| unsupported("the cap perimeter is open"))?;
            visited.insert(cursor);
            cursor = edge.back().id();
            wire.push(edge);
            if cursor == start {
                break;
            }
        }
        loops.push(wire)
    }
    Ok(loops)
}
fn build(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    source: &Definition,
    bodies: &BTreeMap<String, Vec<Solid>>,
) -> Result<(Frame, Vec<Region>, Vec<Vertex>)> {
    let frame = source.frame(doc, e, sheet)?;
    let tol = e.accuracy.modeling.linear_mm;
    let mut regions = vec![];
    let mut vertices = vec![];
    let mut controls = 262144usize;
    let mut work = 0;
    for display in drawing::sheet_display_instances(doc, e, sheet)? {
        use cad_core::display_states::Visibility;
        if display.effect.visibility == Some(Visibility::Hidden) {
            continue;
        }
        let ghost = display.effect.visibility == Some(Visibility::Ghost);
        let instance = display.placement;
        let key = cad_core::history::mesh_key(&instance);
        let mesh = e
            .meshes
            .get(key)
            .ok_or_else(|| unsupported("the displayed component is unavailable"))?;
        let solids = bodies
            .get(key)
            .ok_or_else(|| unsupported("the displayed component has no current solid geometry"))?;
        let mut endpoints = Vec::new();
        for solid in solids {
            if solid.face_iter().take(513).count() > 512
                || solid.edge_iter().take(2049).count() > 2048
            {
                return Err(unsupported(
                    "each cut supports up to 512 faces and 2048 edges per solid",
                ));
            }
            controls -= crate::split::control_cost(solid, controls)?;
            let world = builder::transformed(solid, crate::adapter::matrix(instance.matrix));
            let caps = crate::split::section_faces(&world, &frame, tol)?;
            let mut loops = Vec::new();
            for wire in perimeter(caps)? {
                let mut boundary = Vec::new();
                for edge in wire {
                    let p = edge.front().point();
                    endpoints.push([p.x, p.y, p.z]);
                    let curve =
                        crate::adapter::exact_curve(&edge.oriented_curve()).ok_or_else(|| {
                            unsupported("a cap boundary has no exact rational output carrier")
                        })?;
                    controls = controls
                        .checked_sub(curve.control_points.len())
                        .ok_or_else(|| unsupported("the section exceeds 262144 native controls"))?;
                    if curve
                        .control_points
                        .iter()
                        .any(|p| frame.distance(*p).abs() > tol)
                    {
                        return Err(unsupported("a section boundary leaves the cutting plane"));
                    }
                    boundary.push(curve);
                }
                if !boundary.is_empty() {
                    loops.push(boundary)
                }
            }
            if !loops.is_empty() {
                regions.push(Region { loops, ghost })
            }
        }
        if ghost || endpoints.is_empty() {
            continue;
        }
        endpoints.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for edge in &mesh.edges {
            let Some(local) = edge.curve.as_ref() else {
                continue;
            };
            let curve = cad_core::sketch_edit::Curve3 {
                control_points: local
                    .control_points
                    .iter()
                    .map(|p| at(*p, instance.matrix))
                    .collect(),
                ..local.clone()
            };
            let hits = section::crossings(&curve, &frame, tol, &mut work)?;
            let count = hits.len();
            for (branch, point) in hits.into_iter().enumerate() {
                let first = endpoints.partition_point(|p| p[0] < point[0] - tol);
                if endpoints[first..]
                    .iter()
                    .take_while(|p| p[0] <= point[0] + tol)
                    .any(|p| section::near(*p, point, tol))
                {
                    let source_edge = CurveReference::ModelEdge {
                        edge: edge.reference.clone(),
                        body: edge.body.clone(),
                        instance_path: instance.path.clone(),
                    };
                    vertices.push(Vertex {
                        reference: section::reference(&source.id, source_edge, branch, count),
                        position: point,
                    });
                }
            }
        }
    }
    if regions.is_empty() {
        return Err(unsupported(
            "the cutting line misses the displayed solids. Move it inside a body",
        ));
    }
    Ok((frame, regions, vertices))
}
pub(crate) fn evaluate(
    doc: &Document,
    e: &Evaluation,
    bodies: &BTreeMap<String, Vec<Solid>>,
) -> BTreeMap<String, Evaluated> {
    doc.drawings
        .iter()
        .filter_map(|sheet| {
            let source = sheet.section.as_ref()?;
            let mut cut = Evaluated {
                source: source.clone(),
                frame: None,
                regions: vec![],
                vertices: vec![],
                error: None,
            };
            match build(doc, e, sheet, source, bodies) {
                Ok((frame, regions, vertices)) => {
                    cut.frame = Some(frame);
                    cut.regions = regions;
                    cut.vertices = vertices
                }
                Err(error) => cut.error = Some(error.to_string()),
            }
            Some((sheet.id.clone(), cut))
        })
        .collect()
}
