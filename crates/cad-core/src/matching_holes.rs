//! Associative hole centers in the coordinate frame of a specific assembly occurrence.
use crate::{
    CadError, Document, Id, NodeRef, Plane, Result, TopologyRef, reference_geometry::Axis,
};
use nalgebra::{Matrix4, Point3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub assembly: Id,
    pub target_part: Id,
    pub target_path: Vec<Id>,
    pub centers: BTreeMap<Id, Center>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Center {
    pub path: Vec<Id>,
    pub body: NodeRef,
    pub reference: TopologyRef,
}
impl Center {
    pub fn axis_source(&self) -> Result<crate::reference_geometry::AxisSource> {
        use crate::reference_geometry::AxisSource;
        let reference = self.reference.clone();
        let body = self.body.clone();
        match reference.topology.as_str() {
            "face" => Ok(AxisSource::FaceAxis { reference, body }),
            "edge" => Ok(AxisSource::CircleEdge { reference, body }),
            _ => Err(invalid("Select a circular rim or a cylindrical bore.")),
        }
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Match Holes: {message}"))
}
pub fn validate(doc: &Document) -> Result<()> {
    let mut graph = BTreeMap::<&str, BTreeSet<&str>>::new();
    for part in doc.parts.values() {
        for sketch in part.sketches.values() {
            let Some(link) = &sketch.matched_centers else {
                continue;
            };
            if !doc.assemblies.contains_key(&link.assembly) {
                return Err(invalid("The assembly that owns the alignment is missing."));
            }
            if link.target_part != part.id || link.target_path.is_empty() || link.centers.is_empty()
            {
                return Err(invalid(
                    "A following sketch needs a cover occurrence and at least one source hole.",
                ));
            }
            for (key, center) in &link.centers {
                if !sketch.points.contains_key(key)
                    || center.path.is_empty()
                    || !doc.parts.contains_key(&center.body.definition)
                {
                    return Err(invalid("A linked hole position or source part is missing."));
                }
                if !matches!(center.reference.topology.as_str(), "edge" | "face") {
                    return Err(invalid("Select a circular rim or a cylindrical bore."));
                }
                graph
                    .entry(&part.id)
                    .or_default()
                    .insert(&center.body.definition);
            }
        }
    }
    fn visit<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(node) {
            return Ok(());
        }
        if !active.insert(node) {
            return Err(invalid(
                "These parts would follow each other. Choose Copy once to make independent holes.",
            ));
        }
        for &next in graph.get(node).into_iter().flatten() {
            visit(next, graph, active, done)?;
        }
        active.remove(node);
        done.insert(node);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for &node in graph.keys() {
        visit(node, &graph, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}
pub fn position(
    plane: &Plane,
    target: [[f64; 4]; 4],
    source: [[f64; 4]; 4],
    axis: &Axis,
) -> Result<[f64; 2]> {
    let matrix = |rows: [[f64; 4]; 4]| Matrix4::from_fn(|r, c| rows[r][c]);
    let frame = matrix(target)
        .try_inverse()
        .ok_or_else(|| invalid("Cover placement is singular."))?
        * matrix(source);
    let p = frame.transform_point(&Point3::from(axis.origin));
    let dir = frame
        .transform_vector(&Vector3::from(axis.direction))
        .try_normalize(1e-12)
        .ok_or_else(|| invalid("The bore axis is undefined."))?;
    let u = Vector3::from(plane.u);
    let v = Vector3::from(plane.v);
    let n = u
        .cross(&v)
        .try_normalize(1e-12)
        .ok_or_else(|| invalid("The cover face has no normal."))?;
    if n.cross(&dir).norm() > 1e-6 {
        return Err(invalid("Choose a hole perpendicular to the cover face."));
    }
    let offset = p - Point3::from(plane.origin);
    let on_plane = offset - dir * (offset.dot(&n) / dir.dot(&n));
    let xy = [on_plane.dot(&u), on_plane.dot(&v)];
    if xy.iter().any(|v| !v.is_finite()) {
        return Err(invalid("The projected center is undefined."));
    }
    Ok(xy)
}
