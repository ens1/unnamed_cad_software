use crate::*;
use bevy::math::DVec3;
use bevy::{
    asset::RenderAssetUsages,
    mesh::Indices,
    render::render_resource::{Extent3d, Face, PrimitiveTopology},
};
use bevy_egui::egui;
use cad_core::{
    PlacedPart, TopologyRef,
    protocol::{Evaluation, MeshData, OPERATION_PREVIEW_VERSION},
};
pub(crate) fn style_target(w: &Workspace) -> cad_core::presentation::StyleTarget {
    let [width, height] = w.presentation_resources.logical_size;
    let region = w
        .layout
        .regions(&w.view.active_layout, width.max(1.), height.max(1.))
        .ok()
        .and_then(|rs| rs.into_iter().find(|r| r.role == "model-viewport"))
        .unwrap_or(cad_core::presentation::Region {
            id: "viewport".into(),
            role: "model-viewport".into(),
            component: None,
            rect: [0.; 4],
            classes: vec![],
            resizable: false,
        });
    w.layout
        .style_target(&region, &w.view.active_layout, "ready")
}
pub(crate) fn style(w: &Workspace) -> BTreeMap<String, String> {
    w.theme.properties_for(&style_target(w))
}
fn style_color(props: &BTreeMap<String, String>, key: &str, fallback: Color) -> Color {
    props
        .get(key)
        .and_then(|s| crate::ui::color(s))
        .map(|c| {
            let [r, g, b, a] = c.to_srgba_unmultiplied();
            Color::srgba_u8(r, g, b, a)
        })
        .unwrap_or(fallback)
}
pub(crate) fn selection_color(w: &Workspace) -> egui::Color32 {
    style(w)
        .get("selection-color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(egui::Color32::from_rgb(255, 90, 0))
}
#[derive(Clone, Debug)]
pub struct Selection {
    pub instance: String,
    pub path: Vec<String>,
    pub name: String,
    pub definition: String,
    pub reference: TopologyRef,
    pub body: Option<cad_core::NodeRef>,
    pub distance: f64,
    pub point: [f64; 3],
}
pub fn render_point(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32 * 0.01, p[2] as f32 * 0.01, -p[1] as f32 * 0.01)
}
pub fn world_point(p: Vec3) -> [f64; 3] {
    [p.x as f64 * 100., -p.z as f64 * 100., p.y as f64 * 100.]
}
pub fn placed(p: [f64; 3], i: &PlacedPart) -> [f64; 3] {
    std::array::from_fn(|j| {
        i.matrix[j][0] * p[0] + i.matrix[j][1] * p[1] + i.matrix[j][2] * p[2] + i.matrix[j][3]
    })
}
pub fn eye(w: &Workspace) -> Vec3 {
    let [yaw, pitch] = w.view.orbit;
    Vec3::from_array(w.view.target)
        + Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ) * w.view.distance
}
pub fn basis(w: &Workspace) -> (Vec3, Vec3, Vec3) {
    let forward = (Vec3::from_array(w.view.target) - eye(w)).normalize();
    // Derive the camera frame from orbit orientation, including over the poles.
    // A fixed world-up vector becomes singular at vertical views and flips beyond them.
    let yaw = w.view.orbit[0];
    let right = Vec3::new(yaw.cos(), 0., -yaw.sin());
    let up = right.cross(forward);
    let (sin, cos) = w.view.roll.sin_cos();
    (forward, right * cos + up * sin, up * cos - right * sin)
}
pub fn project(w: &Workspace, rect: egui::Rect, p: [f64; 3]) -> Option<egui::Pos2> {
    let p = render_point(p) - eye(w);
    let (f, r, u) = basis(w);
    let depth = p.dot(f);
    if depth <= 0.001 {
        return None;
    }
    let [horizontal, vertical] = focal(w, rect);
    Some(rect.center() + egui::vec2(p.dot(r) * horizontal / depth, -p.dot(u) * vertical / depth))
}
/// Duplicated regions display the same render texture. Match its stretch when
/// projecting source handles and constructing interaction rays.
pub(crate) fn focal(w: &Workspace, rect: egui::Rect) -> [f32; 2] {
    let aspect = w.viewport_size[0].max(1) as f32 / w.viewport_size[1].max(1) as f32;
    let scale = 0.5 / (std::f32::consts::PI / 8.).tan();
    [
        rect.width().max(1.) / aspect * scale,
        rect.height().max(1.) * scale,
    ]
}
pub fn ray(w: &Workspace, rect: egui::Rect, p: egui::Pos2) -> ([f64; 3], [f64; 3]) {
    let (f, r, u) = basis(w);
    let [horizontal, vertical] = focal(w, rect);
    let d = (f + r * (p.x - rect.center().x) / horizontal - u * (p.y - rect.center().y) / vertical)
        .normalize();
    (world_point(eye(w)), [d.x as f64, -d.z as f64, d.y as f64])
}

/// Display visibility for transient hover guides, using the same station and
/// feature-input mesh as picking. Ray bounds avoid scanning unrelated parts.
/// This tessellated visibility query never establishes a geometric center.
pub(crate) fn visible_at(
    w: &Workspace,
    rect: egui::Rect,
    point: [f64; 3],
    remaining: &mut usize,
) -> bool {
    let Some(screen) = project(w, rect, point) else {
        return false;
    };
    let Some(evaluation) = selection_evaluation(w) else {
        return false;
    };
    let (origin, direction) = ray(w, rect, screen);
    let origin = DVec3::from_array(origin);
    let direction = DVec3::from_array(direction);
    let limit = (DVec3::from_array(point) - origin).dot(direction);
    if !limit.is_finite() || limit <= 0. {
        return false;
    }
    let epsilon = (limit * 1e-5).max(1e-4);
    let displayed = display_evaluation(w).unwrap_or(evaluation);
    for display_instance in display_instances(w, displayed) {
        let input = display_instances(w, evaluation).iter().find(|instance| {
            instance.path == display_instance.path
                && instance.definition == display_instance.definition
        });
        let input_meshes = input
            .map(|instance| selection_meshes(w, evaluation, instance))
            .unwrap_or_default();
        let (instance, meshes) = if let Some(input) = input.filter(|_| !input_meshes.is_empty()) {
            (input, input_meshes)
        } else {
            // Edge-feature picking filters unrelated occurrences; those solids
            // still render and must continue to occlude the input-edge guides.
            (
                display_instance,
                display_meshes(w, displayed, display_instance),
            )
        };
        if !visible(w, instance) || ghost_neighbor(w, instance) {
            continue;
        }
        let from_local =
            bevy::math::DMat4::from_cols_array(&std::array::from_fn::<_, 16, _>(|index| {
                instance.matrix[index % 4][index / 4]
            }));
        let to_local = from_local.inverse();
        if !to_local.is_finite() {
            continue;
        }
        let local_origin = to_local.transform_point3(origin);
        let local_direction = to_local.transform_vector3(direction);
        for mesh in meshes {
            if mesh.indices.is_empty()
                || !ray_bounds(local_origin, local_direction, mesh.bounds, limit)
            {
                continue;
            }
            for indices in mesh.indices.as_chunks::<3>().0 {
                // Optional hover feedback yields conservatively under extreme
                // display work instead of stalling a navigation or drawing gesture.
                if *remaining == 0 {
                    return false;
                }
                *remaining -= 1;
                let [a, b, c] = indices.map(|index| mesh.positions[index as usize]);
                if triangle(local_origin.to_array(), local_direction.to_array(), a, b, c)
                    .is_some_and(|distance| distance < limit - epsilon)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn ray_bounds(origin: DVec3, direction: DVec3, bounds: [[f64; 3]; 2], limit: f64) -> bool {
    let mut near = 0_f64;
    let mut far = limit;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-15 {
            if origin[axis] < bounds[0][axis] || origin[axis] > bounds[1][axis] {
                return false;
            }
        } else {
            let a = (bounds[0][axis] - origin[axis]) / direction[axis];
            let b = (bounds[1][axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return false;
            }
        }
    }
    true
}
fn triangle(o: [f64; 3], d: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let o = DVec3::from_array(o);
    let d = DVec3::from_array(d);
    let a = DVec3::from_array(a);
    let e1 = DVec3::from_array(b) - a;
    let e2 = DVec3::from_array(c) - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-10 {
        return None;
    }
    let inverse = 1. / det;
    let t = o - a;
    let u = t.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = d.dot(q) * inverse;
    if v < 0. || u + v > 1. {
        return None;
    }
    let t = e2.dot(q) * inverse;
    (t > 0.).then_some(t)
}
fn render_mesh(source: &MeshData, instance: &PlacedPart) -> Mesh {
    let positions = source
        .positions
        .iter()
        .map(|point| render_point(placed(*point, instance)).to_array())
        .collect::<Vec<_>>();
    let normals = source
        .normals
        .iter()
        .map(|normal| {
            let normal: [f64; 3] = std::array::from_fn(|axis| {
                instance.matrix[axis][0] * normal[0]
                    + instance.matrix[axis][1] * normal[1]
                    + instance.matrix[axis][2] * normal[2]
            });
            render_point(normal).normalize_or_zero().to_array()
        })
        .collect::<Vec<_>>();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(source.indices.clone()))
}
pub(crate) fn display_evaluation(w: &Workspace) -> Option<&Evaluation> {
    if let Some(evaluation) = crate::reference_repair_ui::display_evaluation(w) {
        return Some(evaluation);
    }
    if !w.view.original
        && w.view.history_cursor.is_none()
        && let Some(evaluation) = crate::assembly_motion_ui::display_evaluation(w)
    {
        return Some(evaluation);
    }
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .filter(|_| !w.view.original || w.view.history_cursor.is_some())
        .or(w.session.evaluation.as_ref())
}
pub(crate) fn display_instances<'a>(w: &Workspace, e: &'a Evaluation) -> &'a [PlacedPart] {
    cad_core::exploded_views::selected(e, w.view.exploded_view.as_ref())
        .map(|stage| stage.instances.as_slice())
        .unwrap_or(&e.instances)
}
pub(crate) fn display_meshes<'a>(
    w: &Workspace,
    e: &'a Evaluation,
    i: &PlacedPart,
) -> Vec<&'a MeshData> {
    let mut meshes = cad_core::history::display_meshes(e, i, w.view.history_cursor.as_ref());
    let key = if i.mesh_key.is_empty() {
        &i.definition
    } else {
        &i.mesh_key
    };
    let history_index = w
        .view
        .history_cursor
        .as_ref()
        .filter(|cursor| cursor.definition == i.definition)
        .and_then(|cursor| cad_core::history::stage(e, key, &cursor.node))
        .map(|stage| stage.index);
    meshes.extend(e.reference_curve_meshes.iter().filter_map(|(id, mesh)| {
        let feature = id.strip_prefix(&format!("{key}::"))?;
        let index = cad_core::history::stage(e, key, feature)?.index;
        history_index
            .is_none_or(|limit| index <= limit)
            .then_some(mesh)
    }));
    meshes
}
pub(crate) fn visible(w: &Workspace, i: &PlacedPart) -> bool {
    !w.view.isolation
        || w.peek
        || w.view.active_part.as_deref() == Some(&i.definition)
        || w.view.pinned.iter().any(|p| i.path.contains(p))
        || w.view.ghost_others
}
pub(crate) fn ghost_neighbor(w: &Workspace, instance: &PlacedPart) -> bool {
    w.view.isolation
        && !w.peek
        && w.view.active_part.as_ref() != Some(&instance.definition)
        && !w.view.pinned.iter().any(|id| instance.path.contains(id))
}
#[derive(Clone, Copy, Debug)]
enum PreviewKind {
    Added,
    Removed,
}
impl PreviewKind {
    fn name(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }
}
struct PreviewMesh<'a> {
    mesh: &'a MeshData,
    instance: &'a PlacedPart,
    kind: PreviewKind,
}
fn preview_meshes(w: &Workspace) -> Vec<PreviewMesh<'_>> {
    if w.view.original || w.view.history_cursor.is_some() {
        return vec![];
    }
    let Some(evaluation) = display_evaluation(w) else {
        return vec![];
    };
    let Some(preview) = crate::operation_preview::displayed(w)
        .filter(|preview| preview.version == OPERATION_PREVIEW_VERSION)
    else {
        return vec![];
    };
    let instances = display_instances(w, evaluation);
    let displayed = instances
        .iter()
        .map(|instance| (&instance.path, cad_core::history::mesh_key(instance)))
        .collect::<std::collections::BTreeSet<_>>();
    let mut meshes = vec![];
    for instance in instances.iter().filter(|instance| visible(w, instance)) {
        let Some(change) = preview.changes.get(cad_core::history::mesh_key(instance)) else {
            continue;
        };
        for (source, kind) in [
            (&change.added, PreviewKind::Added),
            (&change.removed, PreviewKind::Removed),
        ] {
            if let Some(mesh) = source
                .as_ref()
                .filter(|mesh| !mesh.positions.is_empty() && !mesh.indices.is_empty())
            {
                meshes.push(PreviewMesh {
                    mesh,
                    instance,
                    kind,
                });
            }
        }
    }
    // A delete or replacement can remove an occurrence from the result. Keep
    // only its removal cache at the baseline placement, never its complete body.
    for instance in preview.baseline_instances.iter().filter(|instance| {
        visible(w, instance)
            && !displayed.contains(&(&instance.path, cad_core::history::mesh_key(instance)))
    }) {
        if let Some(mesh) = preview
            .changes
            .get(cad_core::history::mesh_key(instance))
            .and_then(|change| change.removed.as_ref())
            .filter(|mesh| !mesh.positions.is_empty() && !mesh.indices.is_empty())
        {
            meshes.push(PreviewMesh {
                mesh,
                instance,
                kind: PreviewKind::Removed,
            });
        }
    }
    meshes
}
fn preview_color(props: &BTreeMap<String, String>, kind: PreviewKind) -> Color {
    let (property, fallback) = match kind {
        PreviewKind::Added => ("preview-added-color", Color::srgba(0.15, 0.85, 0.52, 0.45)),
        PreviewKind::Removed => ("preview-removed-color", Color::srgba(1., 0.35, 0.12, 0.48)),
    };
    style_color(
        props,
        property,
        style_color(props, "preview-color", fallback),
    )
}
#[derive(Debug)]
struct PreviewStyle {
    color: Color,
    edge_width: f32,
}
fn preview_style(
    w: &Workspace,
    preview: &PreviewMesh<'_>,
    viewport: &BTreeMap<String, String>,
) -> Option<PreviewStyle> {
    let kind = preview.kind.name();
    let key = cad_core::history::mesh_key(preview.instance);
    let parent = style_target(w);
    let state = crate::operation_preview::summary(w)
        .map(|(state, _)| state)
        .unwrap_or_else(|| "ready".into());
    let mut target = parent.child(
        &format!("operation-preview:{key}:{kind}"),
        &format!("operation-preview-{kind}"),
        &["viewport-overlay".into(), "operation-preview".into()],
        &state,
    );
    target.node.attributes.insert("mesh-key".into(), key.into());
    target.node.attributes.insert("kind".into(), kind.into());
    let properties = w.theme.properties_for(&target);
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
        || properties
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return None;
    }
    // Generic UI text colors must not erase directional material colors. A
    // semantic role, class, ID or state selector can override the delta color;
    // compare against an ordinary viewport child to recognize that override.
    let generic = w.theme.properties_for(&parent.child(
        "viewport.generic-overlay",
        "viewport-overlay",
        &["viewport-overlay".into()],
        &state,
    ));
    let fallback = preview_color(viewport, preview.kind);
    let color = if properties.get("color") != generic.get("color") {
        style_color(&properties, "color", fallback)
    } else {
        fallback
    };
    let mut color = color.to_srgba();
    color.alpha *= crate::presentation_controls::number(&properties, "opacity", 1.).clamp(0., 1.);
    if ghost_neighbor(w, preview.instance) {
        color.alpha *=
            crate::presentation_controls::number(viewport, "ghost-opacity", 0.13).clamp(0., 1.);
    }
    if color.alpha <= 0. {
        return None;
    }
    let edge_width = crate::presentation_controls::number(
        &properties,
        "stroke-width",
        crate::presentation_controls::number(viewport, "preview-edge-width", 1.25),
    )
    .clamp(0., 8.);
    Some(PreviewStyle {
        color: color.into(),
        edge_width,
    })
}
type PreviewStyleKey = (String, &'static str, bool);
fn preview_style_key(w: &Workspace, preview: &PreviewMesh<'_>) -> PreviewStyleKey {
    (
        cad_core::history::mesh_key(preview.instance).into(),
        preview.kind.name(),
        ghost_neighbor(w, preview.instance),
    )
}
fn preview_styles(
    w: &Workspace,
    previews: &[PreviewMesh<'_>],
    viewport: &BTreeMap<String, String>,
) -> BTreeMap<PreviewStyleKey, Option<PreviewStyle>> {
    let mut styles = BTreeMap::new();
    for preview in previews {
        styles
            .entry(preview_style_key(w, preview))
            .or_insert_with(|| preview_style(w, preview, viewport));
    }
    styles
}
/// Picking and its feedback share the same evaluated input during a feature edit.
/// A newly previewed shell/fillet may have replaced the topology being selected.
pub(crate) fn selection_evaluation(w: &Workspace) -> Option<&Evaluation> {
    let selecting_input = w.dialog.as_deref() == Some("feature")
        && serde_json::from_str::<cad_core::Feature>(&w.json).is_ok_and(|f| {
            matches!(
                f.operation,
                cad_core::Operation::Fillet { .. }
                    | cad_core::Operation::Chamfer { .. }
                    | cad_core::Operation::Shell { .. }
                    | cad_core::Operation::Draft { .. }
                    | cad_core::Operation::DirectEdit { .. }
                    | cad_core::Operation::BodyFilter { .. }
                    | cad_core::Operation::Split { .. }
            )
        });
    let input = selecting_input
        .then_some(w.tool_checkpoint.as_ref())
        .flatten()
        .and_then(|s| {
            s.draft
                .as_ref()
                .and_then(|d| d.preview_evaluation())
                .or(s.evaluation.as_ref())
        });
    input.or_else(|| display_evaluation(w))
}
/// Editing an existing edge feature selects its authored input from the history
/// cache. The checkpoint's final mesh may already contain the old fillet/chamfer.
pub(crate) fn selection_meshes<'a>(
    w: &Workspace,
    e: &'a Evaluation,
    i: &PlacedPart,
) -> Vec<&'a MeshData> {
    if let Some(feature) = crate::selection_ui::edge_feature(w)
        && let Some((body, _)) = crate::selection_ui::edge_selection(&feature)
    {
        if i.definition != body.definition || !crate::selection_ui::edge_occurrence(w, &i.path) {
            return vec![];
        }
        let key = cad_core::history::mesh_key(i);
        if let Some(mesh) = e
            .history
            .get(key)
            .and_then(|history| history.meshes.get(&body.node))
        {
            return vec![mesh.as_ref()];
        }
        // Older evaluations may have no history cache. Only accept a current
        // display mesh that actually belongs to the authored input output.
        return display_meshes(w, e, i)
            .into_iter()
            .filter(|mesh| {
                mesh.edges
                    .iter()
                    .any(|edge| edge.body.as_ref() == Some(body))
            })
            .collect();
    }
    display_meshes(w, e, i)
}
fn selected_instance(selection: &Selection, instance: &PlacedPart) -> bool {
    selection.path == instance.path && selection.definition == instance.definition
}
fn selected_topology(
    selection: &Selection,
    reference: &TopologyRef,
    body: &Option<cad_core::NodeRef>,
) -> bool {
    selection.reference.topology == reference.topology
        && selection.reference.role == reference.role
        && selection.reference.producer == reference.producer
        && selection.body == *body
}
pub fn pick(w: &Workspace, rect: egui::Rect, p: egui::Pos2) -> Vec<Selection> {
    let Some(e) = selection_evaluation(w) else {
        return vec![];
    };
    let (o, d) = ray(w, rect, p);
    let mut hits = vec![];
    for instance in display_instances(w, e) {
        if !visible(w, instance) {
            continue;
        }
        for mesh in selection_meshes(w, e, instance) {
            let key = instance
                .path
                .last()
                .cloned()
                .unwrap_or_else(|| instance.definition.clone());
            for face in &mesh.faces {
                let mut distance = f64::INFINITY;
                for tri in mesh.indices[face.start..face.start + face.count]
                    .as_chunks::<3>()
                    .0
                {
                    let [a, b, c] = [tri[0], tri[1], tri[2]]
                        .map(|v| placed(mesh.positions[v as usize], instance));
                    if let Some(t) = triangle(o, d, a, b, c) {
                        distance = distance.min(t);
                    }
                }
                if distance.is_finite() {
                    hits.push(Selection {
                        instance: key.clone(),
                        path: instance.path.clone(),
                        name: instance.name.clone(),
                        definition: instance.definition.clone(),
                        reference: face.reference.clone(),
                        body: face.body.clone(),
                        distance,
                        point: std::array::from_fn(|i| o[i] + d[i] * distance),
                    });
                }
            }
            // Edges are projected with a forgiving hit target and remain cycleable through occlusion.
            for edge in &mesh.edges {
                let mut best = f32::INFINITY;
                for line in edge.points.windows(2) {
                    if let (Some(a), Some(b)) = (
                        project(w, rect, placed(line[0], instance)),
                        project(w, rect, placed(line[1], instance)),
                    ) {
                        let ab = b - a;
                        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0., 1.);
                        best = best.min((a + ab * t - p).length());
                    }
                }
                if best < 7. {
                    let center = edge
                        .reference
                        .signature
                        .as_ref()
                        .map(|s| s.center)
                        .or_else(|| edge.points.first().copied());
                    let Some(center) = center else { continue };
                    let point = placed(center, instance);
                    let distance = DVec3::from_array(point).distance(DVec3::from_array(o));
                    hits.push(Selection {
                        instance: key.clone(),
                        path: instance.path.clone(),
                        name: instance.name.clone(),
                        definition: instance.definition.clone(),
                        reference: edge.reference.clone(),
                        body: edge.body.clone(),
                        distance: distance - 0.01,
                        point,
                    });
                }
            }
        }
    }
    hits.extend(crate::reference_ui::picks(w, rect, p, e));
    hits.sort_by(|a, b| {
        let edge_a = a.reference.topology == "edge";
        let edge_b = b.reference.topology == "edge";
        edge_b
            .cmp(&edge_a)
            .then_with(|| a.distance.total_cmp(&b.distance))
    });
    hits.dedup_by(|a, b| {
        a.path == b.path
            && a.definition == b.definition
            && a.body == b.body
            && a.reference.role == b.reference.role
            && a.reference.producer == b.reference.producer
            && a.reference.topology == b.reference.topology
    });
    hits
}
pub fn select(w: &mut Workspace, s: Selection) -> bool {
    let feature_editing = w.dialog.as_deref() == Some("feature") && w.tool_checkpoint.is_some();
    if feature_editing && w.view.active_part.as_ref() != Some(&s.definition) {
        w.status = "Select geometry on the part being edited".into();
        return false;
    }
    if let Some(feature) = crate::selection_ui::edge_feature(w)
        && let Some((body, _)) = crate::selection_ui::edge_selection(&feature)
        && (s.body.as_ref() != Some(body) || !crate::selection_ui::edge_occurrence(w, &s.path))
    {
        w.status = "Select edges or faces on this feature's input body and component".into();
        return false;
    }
    // Tab candidates can outlive a worker result. Resolve only a unique native
    // reference on the current occurrence, rather than selecting stale topology.
    if matches!(s.reference.topology.as_str(), "face" | "edge") {
        let matches = selection_evaluation(w)
            .map(|evaluation| {
                display_instances(w, evaluation)
                    .iter()
                    .filter(|instance| selected_instance(&s, instance) && visible(w, instance))
                    .flat_map(|instance| selection_meshes(w, evaluation, instance))
                    .map(|mesh| {
                        if s.reference.topology == "face" {
                            mesh.faces
                                .iter()
                                .filter(|face| selected_topology(&s, &face.reference, &face.body))
                                .count()
                        } else {
                            mesh.edges
                                .iter()
                                .filter(|edge| selected_topology(&s, &edge.reference, &edge.body))
                                .count()
                        }
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0);
        if matches != 1 {
            w.status = "Selection changed during evaluation; select the geometry again".into();
            w.candidates.clear();
            w.selected = None;
            w.geometry_selection.clear();
            w.selected_feature_path = None;
            w.context_selection = None;
            return false;
        }
    }
    let reference = w.view.isolation && w.view.active_part.as_ref() != Some(&s.definition);
    if !reference && !feature_editing {
        w.view.active_part = Some(s.definition.clone());
        w.view.active_instance = s.path.last().cloned();
        w.view.active_node = Some(s.reference.producer.node.clone());
    }
    w.context_selection = None;
    w.selected_feature_path = None;
    if !feature_editing
        || crate::selection_ui::edge_feature(w).is_some() && w.edge_selection_path.is_none()
    {
        w.edge_selection_path = Some(s.path.clone());
    }
    w.geometry_selection = vec![s.clone()];
    w.selected = Some(s);
    w.active_plane = None;
    w.rendered_key.clear();
    crate::navigation::stop(w);
    true
}
/// Command/Shift preselection accumulates edges on one body occurrence. During
/// feature editing the authored edge vector is handled by its draft capture.
pub fn select_modified(w: &mut Workspace, s: Selection, toggle: bool) -> bool {
    let prior = w.geometry_selection.clone();
    let editing = w.tool_checkpoint.is_some();
    if !select(w, s.clone()) {
        return false;
    }
    if toggle && !editing && s.reference.topology == "edge" {
        let mut selected = prior
            .into_iter()
            .filter(|pick| {
                pick.reference.topology == "edge"
                    && pick.path == s.path
                    && pick.definition == s.definition
                    && pick.body == s.body
            })
            .collect::<Vec<_>>();
        if let Some(index) = selected
            .iter()
            .position(|pick| selected_topology(pick, &s.reference, &s.body))
        {
            selected.remove(index);
            w.selected = selected.last().cloned();
        } else {
            selected.push(s);
        }
        w.geometry_selection = selected;
    }
    true
}

pub(crate) fn preselected_edges(w: &Workspace, body: &cad_core::NodeRef) -> Vec<TopologyRef> {
    let Some(selected) = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "edge" && s.body.as_ref() == Some(body))
    else {
        return vec![];
    };
    let mut edges = vec![];
    for pick in w.geometry_selection.iter().chain(std::iter::once(selected)) {
        if pick.path == selected.path
            && pick.body.as_ref() == Some(body)
            && pick.definition == body.definition
            && pick.reference.topology == "edge"
            && !edges
                .iter()
                .any(|edge| crate::selection_ui::same_edge(edge, &pick.reference))
        {
            edges.push(pick.reference.clone());
        }
    }
    edges
}
pub fn fit(w: &mut Workspace, selection: bool) {
    if crate::assembly_motion_ui::dragging(w) {
        return;
    }
    if w.sketch
        .as_ref()
        .is_some_and(|s| s.drag.is_some() || !s.chain.is_empty() || !s.clicks.is_empty())
    {
        return;
    }
    crate::navigation::stop(w);
    let Some(e) = display_evaluation(w) else {
        return;
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let props = style(w);
    for i in display_instances(w, e).iter().filter(|i| visible(w, i)) {
        if selection
            && w.selected.as_ref().map_or_else(
                || w.view.active_part.as_ref() != Some(&i.definition),
                |selected| selected.path != i.path,
            )
        {
            continue;
        }
        for m in display_meshes(w, e, i)
            .into_iter()
            .filter(|m| !m.positions.is_empty())
        {
            for corner in 0..8 {
                let p = std::array::from_fn(|axis| m.bounds[(corner >> axis) & 1][axis]);
                let p = render_point(placed(p, i));
                min = min.min(p);
                max = max.max(p);
            }
        }
        let key = if i.mesh_key.is_empty() {
            &i.definition
        } else {
            &i.mesh_key
        };
        for (_, sketch) in e.sketches.iter().filter(|(id, _)| {
            id.strip_prefix(&format!("{key}::")).is_some_and(|feature| {
                w.view
                    .history_cursor
                    .as_ref()
                    .filter(|c| c.definition == i.definition)
                    .is_none_or(|c| {
                        cad_core::history::stage(e, key, &c.node)
                            .is_some_and(|s| s.sketches.iter().any(|id| id == feature))
                    })
            })
        }) {
            let mut points = sketch.points.values().copied().collect::<Vec<_>>();
            for circle in sketch.circles.values() {
                if let Some(center) = sketch.points.get(&circle.center) {
                    for offset in [
                        [circle.radius_mm, 0.],
                        [-circle.radius_mm, 0.],
                        [0., circle.radius_mm],
                        [0., -circle.radius_mm],
                    ] {
                        points.push([center[0] + offset[0], center[1] + offset[1]]);
                    }
                }
            }
            points.extend(
                sketch
                    .polylines
                    .iter()
                    .flat_map(|p| p.points.iter().copied()),
            );
            for uv in points {
                let point = std::array::from_fn(|axis| {
                    sketch.plane.origin[axis]
                        + sketch.plane.u[axis] * uv[0]
                        + sketch.plane.v[axis] * uv[1]
                });
                let point = render_point(placed(point, i));
                min = min.min(point);
                max = max.max(point);
            }
        }
    }
    for preview in preview_meshes(w) {
        if preview_style(w, &preview, &props).is_none() {
            continue;
        }
        if selection
            && w.selected.as_ref().map_or_else(
                || w.view.active_part.as_ref() != Some(&preview.instance.definition),
                |selected| selected.path != preview.instance.path,
            )
        {
            continue;
        }
        for corner in 0..8 {
            let point = std::array::from_fn(|axis| preview.mesh.bounds[(corner >> axis) & 1][axis]);
            let point = render_point(placed(point, preview.instance));
            min = min.min(point);
            max = max.max(point);
        }
    }
    if min.is_finite() && max.is_finite() {
        w.view.target = ((min + max) * 0.5).to_array();
        w.view.distance = ((max - min).length() * 1.4).max(0.08);
    } else {
        w.view.target = [0.; 3];
        w.view.distance = 3.5;
    }
}
pub fn camera(
    w: Res<Workspace>,
    image: Res<ViewImage>,
    mut images: ResMut<Assets<Image>>,
    mut query: Query<(&mut Transform, &mut Projection, &mut Camera), With<CadCamera>>,
) {
    if let Ok((mut t, mut projection, mut camera)) = query.single_mut() {
        camera.clear_color = ClearColorConfig::Custom(style_color(
            &style(&w),
            "background",
            Color::srgb(0.82, 0.84, 0.86),
        ));
        *t = Transform::from_translation(eye(&w))
            .looking_at(Vec3::from_array(w.view.target), basis(&w).2);
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = std::f32::consts::FRAC_PI_4;
            p.near = (w.view.distance * 0.0001).max(0.00001);
            p.far = (w.view.distance * 100.).max(100.);
        }
    }
    if let Some(mut img) = images.get_mut(&image.image) {
        let size = Extent3d {
            width: w.viewport_size[0].clamp(32, 4096),
            height: w.viewport_size[1].clamp(32, 4096),
            depth_or_array_layers: 1,
        };
        if img.texture_descriptor.size != size {
            img.resize(size);
        }
    }
}
pub fn sync_scene(
    mut commands: Commands,
    mut w: ResMut<Workspace>,
    existing: Query<(Entity, &CadBody)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut finishes: ResMut<Assets<render_appearance::FinishMaterial>>,
    mut appearance: ResMut<render_appearance::Runtime>,
) {
    let props = style(&w);
    let preview_styles = preview_styles(&w, &preview_meshes(&w), &props);
    let key = format!(
        "{}:{}:{}:{}:{}:{}:{:?}:{:?}:{}:{:?}:{:?}:{:?}:{}",
        w.session
            .evaluation
            .as_ref()
            .map(|e| e.identity.source.as_str())
            .unwrap_or(""),
        w.session
            .draft
            .as_ref()
            .and_then(|d| d.preview_evaluation())
            .map(|e| e.identity.source.as_str())
            .unwrap_or(""),
        w.view.original,
        w.view.isolation,
        w.view.ghost_others,
        w.peek,
        w.view.active_part,
        w.view.pinned,
        w.compare
            .as_ref()
            .map(|e| e.identity.source.as_str())
            .unwrap_or(""),
        props,
        (w.view.history_cursor.clone(), w.view.exploded_view.clone()),
        w.selected.as_ref().map(|selection| (
            &selection.definition,
            &selection.path,
            &selection.body,
            &selection.reference.topology,
            &selection.reference.producer,
            &selection.reference.role,
        )),
        selection_evaluation(&w)
            .map(|evaluation| evaluation.identity.source.as_str())
            .unwrap_or("")
    );
    let key = format!(
        "{key}:{}:{}:{:?}:{}:{:?}:{:?}:{preview_styles:?}",
        appearance.epoch,
        w.session.generation,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit)),
        w.fields.contains_key("appearance-tool"),
        crate::operation_preview::focus(&w),
        crate::operation_preview::displayed(&w).map(|preview| (
            &preview.baseline_identity,
            &preview.focus,
            preview.version
        ))
    );
    let key = format!(
        "{key}:{:?}",
        display_evaluation(&w).map(|e| (&e.identity, e.valid))
    );
    if w.rendered_key == key {
        return;
    }
    w.rendered_key = key;
    for (entity, body) in existing.iter() {
        commands.entity(entity).despawn();
        meshes.remove(&body.mesh);
        match &body.material {
            CadMaterial::Standard(material) => {
                materials.remove(material);
            }
            CadMaterial::Finish(material) => {
                finishes.remove(material);
            }
        }
    }
    let root = w.root();
    let mut layers = vec![];
    // The evaluated result supplies the one neutral model. A modeling preview
    // is expressed by localized added/removed caches below, never by layering a
    // complete translucent draft over the complete committed part.
    if let Some(evaluation) = display_evaluation(&w) {
        let document = if w.view.original && w.view.history_cursor.is_none() {
            &w.session.committed
        } else {
            w.session.visible()
        };
        layers.push((evaluation, document, false));
    }
    if w.view.history_cursor.is_none()
        && let (Some(e), Some(d)) = (&w.compare, &w.compare_document)
    {
        layers.push((e, d, true));
    }
    for (e, doc, history) in layers {
        let resolved_finishes = cad_core::appearance::resolved_finishes(doc).unwrap_or_default();
        let default_finish = cad_core::appearance::Finish::default();
        for instance in display_instances(&w, e) {
            if !visible(&w, instance) {
                continue;
            }
            for m in display_meshes(&w, e, instance) {
                if m.positions.is_empty() || m.indices.is_empty() {
                    continue;
                }
                let ghost_neighbor = ghost_neighbor(&w, instance);
                let ghost = history || ghost_neighbor;
                let mesh = meshes.add(render_mesh(m, instance));
                let finish = resolved_finishes
                    .get(&instance.path)
                    .and_then(|id| doc.finishes.get(id))
                    .unwrap_or(&default_finish);
                let c = finish.base_color.map(|value| value as f32);
                let base_color = if history {
                    style_color(&props, "comparison-color", Color::srgba(0.2, 0.4, 1., 0.22))
                } else if ghost_neighbor {
                    let opacity = props
                        .get("ghost-opacity")
                        .and_then(|s| s.parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.13)
                        .clamp(0., 1.);
                    Color::srgba(c[0], c[1], c[2], opacity)
                } else {
                    Color::srgb(c[0], c[1], c[2])
                };
                let has_surfaces = m
                    .faces
                    .iter()
                    .any(|face| face.body_kind == cad_core::protocol::BodyKind::Surface);
                let material = finishes.add(render_appearance::material(
                    doc,
                    finish,
                    instance,
                    &root,
                    &mut appearance,
                    render_appearance::SurfaceStyle {
                        tint: ghost.then_some(base_color),
                        translucent: ghost,
                        two_sided: ghost || has_surfaces,
                    },
                ));
                let mut entity = commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    RenderLayers::layer(1),
                    CadBody {
                        mesh,
                        material: CadMaterial::Finish(material),
                    },
                ));
                if ghost {
                    entity.insert((bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver));
                }
            }
        }
    }
    for preview in preview_meshes(&w) {
        let Some(Some(overlay)) = preview_styles.get(&preview_style_key(&w, &preview)) else {
            continue;
        };
        let mesh = meshes.add(render_mesh(preview.mesh, preview.instance));
        // Added boundaries share the result's exterior and removed boundaries
        // share its cut interface. A depth-biased, untextured overlay gives that
        // localized context without competing opaque bodies or finish maps.
        let two_sided = matches!(preview.kind, PreviewKind::Removed)
            || preview
                .mesh
                .faces
                .iter()
                .any(|face| face.body_kind == cad_core::protocol::BodyKind::Surface);
        let material = materials.add(StandardMaterial {
            base_color: overlay.color,
            metallic: 0.03,
            perceptual_roughness: 0.62,
            alpha_mode: AlphaMode::Blend,
            depth_bias: 2.,
            double_sided: two_sided,
            cull_mode: if two_sided { None } else { Some(Face::Back) },
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            RenderLayers::layer(1),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            CadBody {
                mesh,
                material: CadMaterial::Standard(material),
            },
        ));
    }
    if let Some(selection) = w
        .selected
        .as_ref()
        .filter(|selection| selection.reference.topology == "face")
        && let Some(evaluation) = selection_evaluation(&w)
        && let Some(instance) = display_instances(&w, evaluation)
            .iter()
            .find(|instance| selected_instance(selection, instance) && visible(&w, instance))
    {
        let faces = selection_meshes(&w, evaluation, instance)
            .into_iter()
            .flat_map(|mesh| {
                mesh.faces
                    .iter()
                    .filter(|face| selected_topology(selection, &face.reference, &face.body))
                    .map(move |face| (mesh, face))
            })
            .collect::<Vec<_>>();
        if let [(source, face)] = faces.as_slice() {
            let indices = &source.indices[face.start..face.start + face.count];
            let positions = indices
                .iter()
                .map(|index| {
                    render_point(placed(source.positions[*index as usize], instance)).to_array()
                })
                .collect::<Vec<_>>();
            let normals = indices
                .iter()
                .map(|index| {
                    let normal = source.normals[*index as usize];
                    let normal: [f64; 3] = std::array::from_fn(|axis| {
                        instance.matrix[axis][0] * normal[0]
                            + instance.matrix[axis][1] * normal[1]
                            + instance.matrix[axis][2] * normal[2]
                    });
                    render_point(normal).normalize_or_zero().to_array()
                })
                .collect::<Vec<_>>();
            let mesh = meshes.add(
                Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default(),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
            );
            let [r, g, b, alpha] = selection_color(&w).to_srgba_unmultiplied();
            let opacity = props
                .get("selection-opacity")
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite())
                .unwrap_or(0.32)
                .clamp(0., 1.)
                * alpha as f32
                / 255.;
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(
                    r as f32 / 255.,
                    g as f32 / 255.,
                    b as f32 / 255.,
                    opacity,
                ),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                depth_bias: 2.,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                RenderLayers::layer(1),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
                CadBody {
                    mesh,
                    material: CadMaterial::Standard(material),
                },
            ));
        }
    }
}
pub fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    crate::reference_ui::overlay(w, ui, rect);
    let painter = ui.painter().with_clip_rect(rect);
    let origin = rect.right_bottom() - egui::vec2(58., 66.);
    let (_, right, up) = basis(w);
    for (axis, label, color) in [
        ([1., 0., 0.], "X", egui::Color32::from_rgb(170, 55, 55)),
        ([0., 1., 0.], "Y", egui::Color32::from_rgb(45, 130, 65)),
        ([0., 0., 1.], "Z", egui::Color32::from_rgb(45, 90, 190)),
    ] {
        let vector = render_point(axis).normalize();
        let end = origin + egui::vec2(vector.dot(right), -vector.dot(up)) * 29.;
        painter.line_segment([origin, end], egui::Stroke::new(2., color));
        painter.text(
            end,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.),
            color,
        );
    }
    let props = style(w);
    let accent = selection_color(w);
    let Some(e) = display_evaluation(w) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let previews = preview_meshes(w);
    let styles = preview_styles(w, &previews, &props);
    for preview in previews {
        if let Some(Some(overlay)) = styles.get(&preview_style_key(w, &preview))
            && overlay.edge_width > 0.
        {
            let color = overlay.color.to_srgba();
            let color = egui::Color32::from_rgba_unmultiplied(
                (color.red * 255.) as u8,
                (color.green * 255.) as u8,
                (color.blue * 255.) as u8,
                (color.alpha * 255.) as u8,
            );
            for edge in &preview.mesh.edges {
                for pair in edge.points.windows(2) {
                    if let (Some(a), Some(b)) = (
                        project(w, rect, placed(pair[0], preview.instance)),
                        project(w, rect, placed(pair[1], preview.instance)),
                    ) {
                        painter.line_segment([a, b], egui::Stroke::new(overlay.edge_width, color));
                    }
                }
            }
        }
    }
    if w.view.exploded_view.is_none()
        && w.view.history_cursor.is_none()
        && let Some(highlight) = crate::assembly_inspection_ui::highlight(w, e)
    {
        let first_color = props
            .get("inspection-first-color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(egui::Color32::from_rgb(235, 130, 35));
        let second_color = props
            .get("inspection-second-color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(egui::Color32::from_rgb(45, 150, 210));
        for (path, color, label) in [
            (&highlight.first_path, first_color, "A"),
            (&highlight.second_path, second_color, "B"),
        ] {
            if let Some(instance) = e.instances.iter().find(|instance| &instance.path == path)
                && let Some(mesh) = e.meshes.get(if instance.mesh_key.is_empty() {
                    &instance.definition
                } else {
                    &instance.mesh_key
                })
                && !mesh.positions.is_empty()
            {
                let corners: [_; 8] = std::array::from_fn(|index| {
                    project(
                        w,
                        rect,
                        placed(
                            std::array::from_fn(|axis| mesh.bounds[(index >> axis) & 1][axis]),
                            instance,
                        ),
                    )
                });
                for index in 0..8 {
                    for axis in 0..3 {
                        let other = index ^ (1 << axis);
                        if index < other
                            && let (Some(a), Some(b)) = (corners[index], corners[other])
                        {
                            painter.line_segment([a, b], egui::Stroke::new(2., color));
                        }
                    }
                }
                if let Some(position) = corners[7] {
                    painter.text(
                        position,
                        egui::Align2::LEFT_BOTTOM,
                        label,
                        egui::FontId::proportional(14.),
                        color,
                    );
                }
            }
        }
        if let Some([first, second]) = highlight.witness_mm
            && let (Some(a), Some(b)) = (project(w, rect, first), project(w, rect, second))
        {
            painter.line_segment([a, b], egui::Stroke::new(2., accent));
            painter.circle_filled(a, 4., first_color);
            painter.circle_filled(b, 4., second_color);
            painter.text(
                a.lerp(b, 0.5),
                egui::Align2::CENTER_BOTTOM,
                "Approx. boundary distance",
                egui::FontId::proportional(11.),
                accent,
            );
        }
    }
    if w.view.exploded_view.as_ref().is_some_and(|s| s.show_lines)
        && let Some(stage) = cad_core::exploded_views::selected(e, w.view.exploded_view.as_ref())
    {
        let color = props
            .get("explode-line-color")
            .and_then(|c| crate::ui::color(c))
            .unwrap_or(egui::Color32::from_rgb(110, 110, 110));
        for line in &stage.lines {
            if let (Some(a), Some(b)) = (
                project(w, rect, line.start_mm),
                project(w, rect, line.end_mm),
            ) {
                let length = a.distance(b);
                let count = (length / 8.).ceil().max(1.) as usize;
                for index in 0..count.min(2048) {
                    let start = index as f32 / count as f32;
                    let end = ((index as f32 + 0.5) / count as f32).min(1.);
                    painter.line_segment(
                        [a.lerp(b, start), a.lerp(b, end)],
                        egui::Stroke::new(1., color),
                    );
                }
            }
        }
    }
    let feedback_evaluation = selection_evaluation(w).unwrap_or(e);
    let edge_feature = crate::selection_ui::edge_feature(w);
    let mut marker = None;
    for i in display_instances(w, feedback_evaluation) {
        if !visible(w, i) {
            continue;
        }
        let meshes = selection_meshes(w, feedback_evaluation, i);
        let selected = w.selected.as_ref().filter(|s| {
            selected_instance(s, i)
                && meshes
                    .iter()
                    .map(|mesh| {
                        if s.reference.topology == "face" {
                            mesh.faces
                                .iter()
                                .filter(|face| selected_topology(s, &face.reference, &face.body))
                                .count()
                        } else {
                            mesh.edges
                                .iter()
                                .filter(|edge| selected_topology(s, &edge.reference, &edge.body))
                                .count()
                        }
                    })
                    .sum::<usize>()
                    == 1
        });
        for m in meshes {
            for edge in &m.edges {
                let active = selected.is_some_and(|s| {
                    selected_topology(s, &edge.reference, &edge.body)
                        || (s.reference.topology == "face"
                            && s.body == edge.body
                            && edge.faces.iter().any(|face| {
                                face.producer == s.reference.producer
                                    && face.role == s.reference.role
                                    && face.topology == s.reference.topology
                            }))
                }) || edge_feature.as_ref().is_some_and(|feature| {
                    crate::selection_ui::edge_selection(feature).is_some_and(|(body, edges)| {
                        i.definition == body.definition
                            && w.edge_selection_path
                                .as_ref()
                                .is_some_and(|path| path == &i.path)
                            && edge.body.as_ref() == Some(body)
                            && edges.iter().any(|reference| {
                                crate::selection_ui::same_edge(reference, &edge.reference)
                            })
                    })
                }) || w.selected.as_ref().is_some_and(|primary| {
                    selected_instance(primary, i)
                        && w.geometry_selection.iter().any(|pick| {
                            selected_instance(pick, i)
                                && selected_topology(pick, &edge.reference, &edge.body)
                        })
                });
                if active || m.indices.is_empty() {
                    let curve_color = props
                        .get("reference-curve-color")
                        .and_then(|value| crate::ui::color(value))
                        .unwrap_or(egui::Color32::from_rgb(65, 130, 185));
                    let stroke = egui::Stroke::new(
                        if active { 3.5 } else { 1.5 },
                        if active { accent } else { curve_color },
                    );
                    for pair in edge.points.windows(2) {
                        if let (Some(a), Some(b)) = (
                            project(w, rect, placed(pair[0], i)),
                            project(w, rect, placed(pair[1], i)),
                        ) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                    if selected.is_some_and(|s| selected_topology(s, &edge.reference, &edge.body)) {
                        marker = edge
                            .reference
                            .signature
                            .as_ref()
                            .map(|signature| signature.center)
                            .or_else(|| edge.points.get(edge.points.len() / 2).copied())
                            .map(|point| placed(point, i));
                    }
                }
            }
            if let Some(s) = selected.filter(|s| s.reference.topology == "face") {
                for face in m
                    .faces
                    .iter()
                    .filter(|f| selected_topology(s, &f.reference, &f.body))
                {
                    marker = face
                        .reference
                        .signature
                        .as_ref()
                        .map(|signature| placed(signature.center, i));
                }
            }
        }
    }
    if let Some(point) = marker
        && let Some(p) = project(w, rect, point)
    {
        painter.circle_stroke(p, 6., egui::Stroke::new(1.5, accent));
    }
    let label_at = |point: egui::Pos2, label: &str, color: egui::Color32, offset: egui::Vec2| {
        let size = egui::vec2(
            (label.chars().count() as f32 * 6.5 + 16.)
                .min(rect.width() - 16.)
                .max(40.),
            26.,
        );
        let preferred = point + offset;
        let min = egui::pos2(
            preferred.x.clamp(
                rect.left() + 8.,
                (rect.right() - size.x - 8.).max(rect.left() + 8.),
            ),
            preferred.y.clamp(
                rect.top() + 8.,
                (rect.bottom() - size.y - 8.).max(rect.top() + 8.),
            ),
        );
        let box_rect = egui::Rect::from_min_size(min, size);
        let end = egui::pos2(box_rect.left(), box_rect.center().y);
        let bend = egui::pos2(end.x - 12., end.y);
        painter.add(egui::Shape::line(
            vec![point, bend, end],
            egui::Stroke::new(1.4, color),
        ));
        painter.circle(
            point,
            4.,
            ui.visuals().panel_fill,
            egui::Stroke::new(1.6, color),
        );
        painter.rect_filled(box_rect, 3., ui.visuals().panel_fill);
        painter.rect_stroke(
            box_rect,
            3.,
            egui::Stroke::new(1., color),
            egui::StrokeKind::Inside,
        );
        painter.text(
            box_rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.),
            ui.visuals().text_color(),
        );
    };
    if let Some(selected) = &w.selected
        && let Some(point) = project(w, rect, selected.point)
    {
        let feature = w
            .view
            .active_part
            .as_ref()
            .and_then(|id| w.session.visible().parts.get(id))
            .and_then(|p| {
                w.view
                    .active_node
                    .as_ref()
                    .and_then(|id| p.features.iter().find(|f| &f.id == id))
            });
        let text = feature
            .map(|f| crate::cad_ui::operation_summary(&f.operation))
            .unwrap_or_else(|| {
                selected
                    .reference
                    .signature
                    .as_ref()
                    .map(|s| {
                        format!(
                            "{} · {:.3} {}",
                            selected.reference.topology,
                            s.measure,
                            if selected.reference.topology == "edge" {
                                "mm"
                            } else {
                                "mm²"
                            }
                        )
                    })
                    .unwrap_or_else(|| selected.reference.topology.clone())
            });
        label_at(point, &text, accent, egui::vec2(44., -52.));
    } else if let Some(node) = &w.view.active_node
        && let Some(part) = w
            .view
            .active_part
            .as_ref()
            .and_then(|id| w.session.visible().parts.get(id))
        && let Some(feature) = part.features.iter().find(|f| &f.id == node)
        && let Some((instance, face)) = display_instances(w, e)
            .iter()
            .filter(|i| i.definition == part.id)
            .find_map(|i| {
                display_meshes(w, e, i)
                    .into_iter()
                    .find_map(|m| m.faces.iter().find(|f| f.reference.producer.node == *node))
                    .map(|f| (i, f))
            })
        && let Some(signature) = &face.reference.signature
        && let Some(point) = project(w, rect, placed(signature.center, instance))
    {
        label_at(
            point,
            &crate::cad_ui::operation_summary(&feature.operation),
            accent,
            egui::vec2(44., -52.),
        );
    }
    let reference_color = w
        .theme
        .variables
        .get("--reference")
        .and_then(|c| crate::ui::color(c))
        .unwrap_or(egui::Color32::from_rgb(0, 135, 190));
    for (index, instance) in display_instances(w, e)
        .iter()
        .filter(|i| w.view.pinned.iter().any(|id| i.path.contains(id)))
        .enumerate()
    {
        if let Some(mesh) = display_meshes(w, e, instance).into_iter().next() {
            let bounds = mesh.bounds;
            let center = std::array::from_fn(|axis| (bounds[0][axis] + bounds[1][axis]) * 0.5);
            if let Some(point) = project(w, rect, placed(center, instance)) {
                label_at(
                    point,
                    &format!("Pinned reference · {}", instance.name),
                    reference_color,
                    egui::vec2(48., 32. + index as f32 * 32.),
                );
            }
        }
    }
}
pub fn navigate(w: &mut Workspace, ctx: &egui::Context, response: &egui::Response) -> bool {
    crate::navigation::navigate(w, ctx, response)
}

/// A viewport overlay, independent of authoritative camera/model definitions.
pub fn view_cube(w: &mut Workspace, ui: &egui::Ui, viewport: egui::Rect) -> egui::Rect {
    let Some(rect) = crate::presentation_controls::overlay_rect(
        "viewport.view-cube",
        "view-cube",
        viewport,
        egui::Rect::from_min_size(
            viewport.right_top() + egui::vec2(-108., 98.),
            egui::vec2(96., 100.),
        ),
    ) else {
        return egui::Rect::NOTHING;
    };
    view_cube_at(w, ui, rect, viewport)
}

pub(crate) fn view_cube_at(
    w: &mut Workspace,
    ui: &egui::Ui,
    rect: egui::Rect,
    clip: egui::Rect,
) -> egui::Rect {
    if !rect.is_positive() {
        return egui::Rect::NOTHING;
    }
    let props = crate::presentation_controls::properties(
        "viewport.view-cube",
        "view-cube",
        &["viewport-overlay"],
        "ready",
    );
    let font = egui::FontId::proportional(crate::presentation_controls::number(
        &props,
        "font-size",
        9.,
    ));
    let ink = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(ui.visuals().text_color());
    let scale = rect.width().min(rect.height()) / 96.;
    let center = rect.center() - egui::vec2(0., 12. * scale);
    let (forward, right, up) = basis(w);
    let painter = ui.painter().with_clip_rect(clip);
    let screen = |p: [f64; 3]| {
        let p = render_point(p) * 100.;
        center + egui::vec2(p.dot(right), -p.dot(up)) * 28. * scale
    };
    let pointer = ui.input(|i| i.pointer.hover_pos());
    for (label, normal, corners, orbit) in [
        (
            "Front",
            [0., -1., 0.],
            [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
            [0., 0.],
        ),
        (
            "Back",
            [0., 1., 0.],
            [[1., 1., -1.], [-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.]],
            [std::f32::consts::PI, 0.],
        ),
        (
            "Right",
            [1., 0., 0.],
            [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]],
            [std::f32::consts::FRAC_PI_2, 0.],
        ),
        (
            "Left",
            [-1., 0., 0.],
            [
                [-1., 1., -1.],
                [-1., -1., -1.],
                [-1., -1., 1.],
                [-1., 1., 1.],
            ],
            [-std::f32::consts::FRAC_PI_2, 0.],
        ),
        (
            "Top",
            [0., 0., 1.],
            [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
            [0., std::f32::consts::FRAC_PI_2],
        ),
        (
            "Bottom",
            [0., 0., -1.],
            [
                [-1., 1., -1.],
                [1., 1., -1.],
                [1., -1., -1.],
                [-1., -1., -1.],
            ],
            [0., -std::f32::consts::FRAC_PI_2],
        ),
    ] {
        let facing = render_point(normal).normalize().dot(-forward);
        if facing < 0.01 {
            continue;
        }
        let points = corners.map(screen);
        let inside = pointer.is_some_and(|q| {
            let signs = (0..4)
                .map(|i| {
                    let a = points[i];
                    let b = points[(i + 1) % 4];
                    (b.x - a.x) * (q.y - a.y) - (b.y - a.y) * (q.x - a.x)
                })
                .collect::<Vec<_>>();
            signs.iter().all(|v| *v >= 0.) || signs.iter().all(|v| *v <= 0.)
        });
        let bound = egui::Rect::from_points(&points);
        let hit = ui.interact(
            bound,
            ui.id().with(("view-cube", label)),
            egui::Sense::click(),
        );
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            if inside {
                ui.visuals().selection.stroke.color.gamma_multiply(0.3)
            } else {
                props
                    .get("background")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(egui::Color32::from_gray((150. + 65. * facing) as u8))
            },
            egui::Stroke::new(
                crate::presentation_controls::number(&props, "stroke-width", 1.),
                ink,
            ),
        ));
        let position = egui::pos2(
            points.iter().map(|p| p.x).sum::<f32>() / 4.,
            points.iter().map(|p| p.y).sum::<f32>() / 4.,
        );
        if facing > 0.25 {
            painter.text(
                position,
                egui::Align2::CENTER_CENTER,
                label,
                font.clone(),
                ink,
            );
        }
        // Face bounding rectangles overlap at oblique views; the convex hit is
        // authoritative, rather than the native rectangular response's winner.
        if inside
            && ui.input(|i| i.pointer.primary_clicked())
            && !crate::assembly_motion_ui::dragging(w)
        {
            w.view.orbit = orbit;
            w.view.roll = 0.;
            crate::navigation::stop(w);
        }
        hit.on_hover_text(format!("{label} view"));
    }
    let iso = egui::Rect::from_center_size(
        rect.center_bottom() - egui::vec2(0., 6. * scale),
        egui::vec2(52., 18.) * scale,
    );
    let hit = ui.interact(
        iso,
        ui.id().with("view-cube-isometric"),
        egui::Sense::click(),
    );
    painter.text(
        iso.center(),
        egui::Align2::CENTER_CENTER,
        "Isometric",
        font,
        ink,
    );
    if hit.clicked() && !crate::assembly_motion_ui::dragging(w) {
        w.view.orbit = [0.65, 0.4];
        w.view.roll = 0.;
        crate::navigation::stop(w);
    }
    rect
}
