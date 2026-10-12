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
pub fn placed(p: [f64; 3], i: &PlacedPart) -> [f64; 3] {
    std::array::from_fn(|j| {
        i.matrix[j][0] * p[0] + i.matrix[j][1] * p[1] + i.matrix[j][2] * p[2] + i.matrix[j][3]
    })
}
fn direction(value: [f64; 3]) -> Vec3 {
    Vec3::new(value[0] as f32, value[2] as f32, -value[1] as f32)
}
pub fn eye(w: &Workspace) -> Vec3 {
    render_point(crate::camera_views_ui::current(w).frame().eye_mm)
}
pub fn basis(w: &Workspace) -> (Vec3, Vec3, Vec3) {
    let frame = crate::camera_views_ui::current(w).frame();
    (
        direction(frame.forward),
        direction(frame.right),
        direction(frame.up),
    )
}
/// Pixel focal lengths, or pixels/mm for an orthographic source camera.
pub(crate) fn focal(w: &Workspace, rect: egui::Rect) -> [f64; 2] {
    let pose = crate::camera_views_ui::current(w);
    let aspect = f64::from(w.viewport_size[0].clamp(32, 4096))
        / f64::from(w.viewport_size[1].clamp(32, 4096));
    let scale = match pose.projection {
        cad_core::camera_views::Projection::Perspective { vertical_fov_deg } => {
            0.5 / (vertical_fov_deg.to_radians() * 0.5).tan()
        }
        cad_core::camera_views::Projection::Orthographic { vertical_span_mm } => {
            1. / vertical_span_mm
        }
    };
    [
        f64::from(rect.width().max(1.)) / aspect * scale,
        f64::from(rect.height().max(1.)) * scale,
    ]
}
pub fn project(w: &Workspace, rect: egui::Rect, p: [f64; 3]) -> Option<egui::Pos2> {
    let pose = crate::camera_views_ui::current(w);
    let frame = pose.frame();
    let offset = DVec3::from_array(p) - DVec3::from_array(frame.eye_mm);
    let depth = offset.dot(DVec3::from_array(frame.forward));
    if !depth.is_finite()
        || depth <= (pose.distance_mm * 0.0001).max(0.001)
        || depth > (pose.distance_mm * 100.).max(10000.)
    {
        return None;
    }
    let divisor = if matches!(
        pose.projection,
        cad_core::camera_views::Projection::Perspective { .. }
    ) {
        depth
    } else {
        1.
    };
    let [horizontal, vertical] = focal(w, rect);
    let x = offset.dot(DVec3::from_array(frame.right)) * horizontal / divisor;
    let y = -offset.dot(DVec3::from_array(frame.up)) * vertical / divisor;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(rect.center() + egui::vec2(x as f32, y as f32))
}
pub fn ray(w: &Workspace, rect: egui::Rect, p: egui::Pos2) -> ([f64; 3], [f64; 3]) {
    let pose = crate::camera_views_ui::current(w);
    let frame = pose.frame();
    let [horizontal, vertical] = focal(w, rect);
    let offset = DVec3::from_array(frame.right) * f64::from(p.x - rect.center().x) / horizontal
        - DVec3::from_array(frame.up) * f64::from(p.y - rect.center().y) / vertical;
    let forward = DVec3::from_array(frame.forward);
    match pose.projection {
        cad_core::camera_views::Projection::Perspective { .. } => {
            (frame.eye_mm, (forward + offset).normalize().to_array())
        }
        cad_core::camera_views::Projection::Orthographic { .. } => (
            (DVec3::from_array(frame.eye_mm) + offset).to_array(),
            frame.forward,
        ),
    }
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
    crate::overlay_depth::visible(w, rect, point, remaining)
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
/// Display batches retain body ownership while allowing independent optical bead finishes.
fn finish_batches<'a>(
    source: &'a MeshData,
    part: Option<&cad_core::Part>,
    face_finishes: &cad_core::appearance::FaceFinishResolution,
) -> Vec<(std::borrow::Cow<'a, MeshData>, Option<String>)> {
    let assignments = part
        .map(|part| {
            part.features
                .iter()
                .filter_map(|feature| match &feature.operation {
                    cad_core::Operation::WeldBead { definition } => definition
                        .finish
                        .as_ref()
                        .map(|finish| (feature.id.as_str(), finish.clone())),
                    _ => None,
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    if assignments.is_empty() && face_finishes.resolved.is_empty() {
        return vec![(std::borrow::Cow::Borrowed(source), None)];
    }
    let mut canonical = cad_core::appearance::FaceFinishIdentityCache::default();
    let overrides = face_finishes
        .resolved
        .iter()
        .filter_map(|assignment| {
            canonical
                .key(&assignment.face)
                .ok()
                .map(|key| ((&assignment.body, key), &assignment.finish))
        })
        .collect::<BTreeMap<_, _>>();
    let mut groups: BTreeMap<Option<String>, Vec<&cad_core::protocol::FaceRange>> = BTreeMap::new();
    for face in &source.faces {
        let finish = face.body.as_ref().and_then(|body| {
            canonical
                .key(&face.reference)
                .ok()
                .and_then(|key| overrides.get(&(body, key)))
                .map(|finish| (*finish).clone())
                .or_else(|| assignments.get(body.node.as_str()).cloned())
        });
        groups.entry(finish).or_default().push(face);
    }
    groups
        .into_iter()
        .map(|(finish, faces)| {
            // Copy only this optical batch's vertices. Cloning the complete
            // part for every finish scales badly for many painted faces.
            let mut mesh = MeshData {
                positions: vec![],
                normals: vec![],
                indices: vec![],
                faces: vec![],
                edges: vec![],
                bounds: source.bounds,
                volume_mm3: source.volume_mm3,
                body_counts: None,
            };
            let mut vertex_map = BTreeMap::new();
            let mut positions = vec![];
            let mut normals = vec![];
            for face in faces {
                let mut range = face.clone();
                range.start = mesh.indices.len();
                for index in &source.indices[face.start..face.start + face.count] {
                    let next = vertex_map.len() as u32;
                    let mapped = *vertex_map.entry(*index).or_insert_with(|| {
                        positions.push(source.positions[*index as usize]);
                        normals.push(source.normals[*index as usize]);
                        next
                    });
                    mesh.indices.push(mapped);
                }
                mesh.faces.push(range);
            }
            mesh.positions = positions;
            mesh.normals = normals;
            (std::borrow::Cow::Owned(mesh), finish)
        })
        .collect()
}
fn normal_matrix(instance: &PlacedPart) -> bevy::math::DMat3 {
    bevy::math::DMat3::from_cols(
        DVec3::new(
            instance.matrix[0][0],
            instance.matrix[1][0],
            instance.matrix[2][0],
        ),
        DVec3::new(
            instance.matrix[0][1],
            instance.matrix[1][1],
            instance.matrix[2][1],
        ),
        DVec3::new(
            instance.matrix[0][2],
            instance.matrix[1][2],
            instance.matrix[2][2],
        ),
    )
    .inverse()
    .transpose()
}
fn render_mesh(source: &MeshData, instance: &PlacedPart) -> Mesh {
    let positions = source
        .positions
        .iter()
        .map(|point| render_point(placed(*point, instance)).to_array())
        .collect::<Vec<_>>();
    let normal_matrix = normal_matrix(instance);
    let normals = source
        .normals
        .iter()
        .map(|normal| {
            render_point((normal_matrix * DVec3::from_array(*normal)).to_array())
                .normalize_or_zero()
                .to_array()
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
pub(crate) fn display_instances<'a>(w: &Workspace, e: &'a Evaluation) -> Vec<&'a PlacedPart> {
    if let Some(stage) = cad_core::exploded_views::selected(e, w.view.exploded_view.as_ref()) {
        return stage.instances.iter().collect();
    }
    e.instances
        .iter()
        .chain(
            e.assembly_welds
                .values()
                .filter(|_| w.view.history_cursor.is_none())
                .map(|weld| &weld.placement),
        )
        .collect()
}
pub(crate) fn display_meshes<'a>(
    w: &'a Workspace,
    e: &'a Evaluation,
    i: &PlacedPart,
) -> Vec<&'a MeshData> {
    let mut meshes = crate::display_mesh::replacement(w, e, i)
        .map(|mesh| vec![mesh])
        .unwrap_or_else(|| cad_core::history::display_meshes(e, i, w.view.history_cursor.as_ref()));
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
    if crate::display_states_ui::effect(w, &i.path).visibility
        == Some(cad_core::display_states::Visibility::Hidden)
    {
        return false;
    }
    !w.view.isolation
        || w.peek
        || crate::assembly_motion_ui::component_selected(w, &i.path)
        || active_context(w, i)
        || w.view.pinned.iter().any(|p| i.path.contains(p))
        || w.view.ghost_others
}
pub(crate) fn ghost_neighbor(w: &Workspace, instance: &PlacedPart) -> bool {
    crate::display_states_ui::effect(w, &instance.path).visibility
        == Some(cad_core::display_states::Visibility::Ghost)
        || w.view.isolation
            && !w.peek
            && !crate::assembly_motion_ui::component_selected(w, &instance.path)
            && !active_context(w, instance)
            && !w.view.pinned.iter().any(|id| instance.path.contains(id))
}
pub(crate) fn active_context(w: &Workspace, instance: &PlacedPart) -> bool {
    w.view.active_part.as_ref() == Some(&instance.definition)
        && w.edge_selection_path
            .as_ref()
            .is_none_or(|path| path == &instance.path)
        && w.view
            .active_instance
            .as_ref()
            .is_none_or(|id| instance.path.last() == Some(id))
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
    let change_for = |instance: &PlacedPart| {
        preview
            .placement_changes_valid
            .then(|| preview.occurrence_changes.get(&instance.path.join("/")))
            .flatten()
            .or_else(|| preview.changes.get(cad_core::history::mesh_key(instance)))
    };
    let mut meshes = vec![];
    for instance in instances
        .iter()
        .copied()
        .filter(|instance| visible(w, instance))
    {
        let Some(change) = change_for(instance) else {
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
        if let Some(mesh) = change_for(instance)
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
    if let Some(evaluation) = crate::reference_repair_ui::display_evaluation(w) {
        return Some(evaluation);
    }
    if !w.view.original
        && w.view.history_cursor.is_none()
        && let Some(evaluation) = crate::assembly_motion_ui::display_evaluation(w)
    {
        return Some(evaluation);
    }
    let selecting_input = crate::fabrication_sources_ui::input_body(w).is_some()
        || w.fields.contains_key("assembly-weld-editor")
        || w.dialog.as_deref() == Some("feature")
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
                        | cad_core::Operation::WeldBead { .. }
                )
            });
    if selecting_input && let Some(checkpoint) = w.tool_checkpoint.as_ref() {
        return checkpoint
            .draft
            .as_ref()
            .and_then(|d| d.preview_evaluation())
            .or(checkpoint.evaluation.as_ref())
            .filter(|e| e.valid && e.identity == checkpoint.identity());
    }
    // Last-valid draft geometry supports navigation, but cannot establish new source references.
    display_evaluation(w).filter(|e| e.valid && e.identity == w.session.identity())
}
/// Editing an existing edge feature selects its authored input from the history
/// cache. The checkpoint's final mesh may already contain the old fillet/chamfer.
pub(crate) fn selection_meshes<'a>(
    w: &'a Workspace,
    e: &'a Evaluation,
    i: &PlacedPart,
) -> Vec<&'a MeshData> {
    if let Some(body) = crate::fabrication_sources_ui::input_body(w) {
        if i.definition != body.definition || !crate::selection_ui::edge_occurrence(w, &i.path) {
            return vec![];
        }
        if let Some(mesh) = e
            .history
            .get(cad_core::history::mesh_key(i))
            .and_then(|history| history.meshes.get(&body.node))
        {
            return vec![mesh.as_ref()];
        }
        return display_meshes(w, e, i)
            .into_iter()
            .filter(|mesh| {
                mesh.edges
                    .iter()
                    .any(|edge| edge.body.as_ref() == Some(&body))
            })
            .collect();
    }
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
        if !visible(w, instance) || !crate::display_states_ui::pickable(w, &instance.path) {
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
                let point = std::array::from_fn(|i| o[i] + d[i] * distance);
                if distance.is_finite() && project(w, rect, point).is_some() {
                    hits.push(Selection {
                        instance: key.clone(),
                        path: instance.path.clone(),
                        name: instance.name.clone(),
                        definition: instance.definition.clone(),
                        reference: face.reference.clone(),
                        body: face.body.clone(),
                        distance,
                        point,
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
                    .into_iter()
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
    let reference = w.view.isolation
        && (w.view.active_part.as_ref() != Some(&s.definition)
            || w.edge_selection_path
                .as_ref()
                .is_some_and(|path| path != &s.path));
    if !reference && !feature_editing {
        w.view.active_part = Some(s.definition.clone());
        w.view.active_instance = s.path.last().cloned();
        w.view.active_node = Some(s.reference.producer.node.clone());
    }
    w.context_selection = None;
    w.selected_feature_path = None;
    if !reference
        && (!feature_editing
            || crate::selection_ui::edge_feature(w).is_some() && w.edge_selection_path.is_none())
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
/// Assembly preselection retains distinct component geometry. Part edge tools
/// filter the retained picks to their input body and occurrence.
pub fn select_modified(w: &mut Workspace, s: Selection, toggle: bool) -> bool {
    let prior = w.geometry_selection.clone();
    let editing = w.tool_checkpoint.is_some();
    if !select(w, s.clone()) {
        return false;
    }
    if toggle
        && !editing
        && w.session
            .visible()
            .assemblies
            .contains_key(&w.session.visible().root)
        && w.session.visible().parts.contains_key(&s.definition)
        && !s.path.is_empty()
        && matches!(s.reference.topology.as_str(), "edge" | "face")
    {
        let mut selected = prior
            .into_iter()
            .filter(|pick| {
                !pick.path.is_empty()
                    && matches!(pick.reference.topology.as_str(), "edge" | "face")
                    && w.session.visible().parts.contains_key(&pick.definition)
            })
            .collect::<Vec<_>>();
        if let Some(index) = selected.iter().position(|pick| {
            pick.path == s.path
                && pick.definition == s.definition
                && selected_topology(pick, &s.reference, &s.body)
        }) {
            selected.remove(index);
            w.selected = selected.last().cloned();
        } else if selected.len() < 1024 {
            selected.push(s);
        }
        w.geometry_selection = selected;
        return true;
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
#[derive(Clone, Copy)]
enum FitScope {
    All,
    Selection,
    Context,
}
pub fn fit(w: &mut Workspace, selection: bool) {
    fit_region(
        w,
        if selection {
            FitScope::Selection
        } else {
            FitScope::All
        },
    );
}
fn fit_context(w: &mut Workspace) {
    fit_region(w, FitScope::Context);
}
/// Disposable framing points for one explicitly placed editing occurrence.
/// Matching a support reference narrows this to its face; it never selects a
/// different occurrence of a shared part or fits the surrounding assembly.
pub(crate) fn occurrence_framing_points(
    w: &Workspace,
    definition: &str,
    matrix: &[[f64; 4]; 4],
    support: Option<(&TopologyRef, Option<&cad_core::NodeRef>)>,
) -> Vec<[f64; 3]> {
    let Some(evaluation) = display_evaluation(w) else {
        return vec![];
    };
    let mut points = Vec::new();
    for instance in display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| instance.definition == definition && &instance.matrix == matrix)
    {
        let mut meshes = display_meshes(w, evaluation, instance);
        if let Some((_, Some(body))) = support
            && let Some(mesh) = evaluation
                .history
                .get(cad_core::history::mesh_key(instance))
                .filter(|history| history.version == cad_core::history::CONTRACT_VERSION)
                .and_then(|history| history.meshes.get(&body.node))
        {
            // Existing face-supported sketches can refer to an earlier stock
            // feature that is no longer a live output. Its disposable history
            // mesh supplies framing without rebuilding or changing that reference.
            meshes.push(mesh.as_ref());
        }
        for mesh in meshes {
            if let Some((reference, body)) = support {
                for face in &mesh.faces {
                    if body.is_some_and(|body| face.body.as_ref() != Some(body))
                        || !cad_core::provenance::equivalent(&face.reference, reference)
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    if let Some(indices) = face
                        .start
                        .checked_add(face.count)
                        .and_then(|end| mesh.indices.get(face.start..end))
                    {
                        points.extend(indices.iter().filter_map(|index| {
                            mesh.positions
                                .get(*index as usize)
                                .map(|point| placed(*point, instance))
                        }));
                    }
                }
            } else if !mesh.indices.is_empty() {
                points.extend((0..8).map(|corner| {
                    placed(
                        std::array::from_fn(|axis| mesh.bounds[(corner >> axis) & 1][axis]),
                        instance,
                    )
                }));
            }
        }
    }
    points
}

/// Frame renderer hints using the current application-owned f64 camera pose.
/// `expand_only` preserves an intentional close view when the points already
/// fit. Source sketches, placements and support references are never changed.
pub(crate) fn frame_points(
    w: &mut Workspace,
    points: &[[f64; 3]],
    target: Option<[f64; 3]>,
    expand_only: bool,
) {
    let points = points
        .iter()
        .map(|point| DVec3::from_array(*point))
        .filter(|point| point.is_finite())
        .collect::<Vec<_>>();
    if points.is_empty() {
        return;
    }
    let mut pose = crate::camera_views_ui::current(w);
    let target = target.map(DVec3::from_array).unwrap_or_else(|| {
        let min = points
            .iter()
            .copied()
            .fold(DVec3::splat(f64::INFINITY), DVec3::min);
        let max = points
            .iter()
            .copied()
            .fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
        (min + max) * 0.5
    });
    pose.target_mm = target.to_array();
    let frame = pose.frame();
    let right = DVec3::from_array(frame.right);
    let up = DVec3::from_array(frame.up);
    let forward = DVec3::from_array(frame.forward);
    let aspect = f64::from(w.viewport_size[0].clamp(32, 4096))
        / f64::from(w.viewport_size[1].clamp(32, 4096));
    match &mut pose.projection {
        cad_core::camera_views::Projection::Perspective { vertical_fov_deg } => {
            let tangent = (vertical_fov_deg.to_radians() * 0.5).tan();
            let distance = points.iter().fold(0.5_f64, |distance, point| {
                let delta = *point - target;
                let required = (delta.dot(up).abs().max(delta.dot(right).abs() / aspect) * 1.18
                    / tangent)
                    - delta.dot(forward);
                distance.max(required).max(-delta.dot(forward) + 0.5)
            });
            pose.distance_mm = if expand_only {
                pose.distance_mm.max(distance)
            } else {
                distance
            }
            .clamp(0.5, 1e6);
        }
        cad_core::camera_views::Projection::Orthographic { vertical_span_mm } => {
            let span = points.iter().fold(0.1_f64, |span, point| {
                let delta = *point - target;
                span.max(2.36 * delta.dot(up).abs().max(delta.dot(right).abs() / aspect))
            });
            *vertical_span_mm = if expand_only {
                vertical_span_mm.max(span)
            } else {
                span
            }
            .clamp(0.1, 1e8);
            pose.distance_mm = points.iter().fold(pose.distance_mm, |distance, point| {
                distance.max(-(*point - target).dot(forward) + 0.5)
            });
        }
    }
    crate::camera_views_ui::set_current(w, pose);
    crate::navigation::stop(w);
}
fn in_fit_scope(w: &Workspace, instance: &PlacedPart, scope: FitScope) -> bool {
    match scope {
        FitScope::All => true,
        FitScope::Context => w.view.active_part.as_ref().is_none_or(|definition| {
            definition == &instance.definition
                && w.edge_selection_path
                    .as_ref()
                    .is_none_or(|path| path == &instance.path)
        }),
        FitScope::Selection => {
            w.geometry_selection
                .iter()
                .any(|pick| selected_instance(pick, instance))
                || w.selected.as_ref().map_or_else(
                    || w.view.active_part.as_ref() == Some(&instance.definition),
                    |selected| selected.path == instance.path,
                )
        }
    }
}
fn fit_region(w: &mut Workspace, scope: FitScope) {
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
        if !in_fit_scope(w, i, scope) {
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
        if !in_fit_scope(w, preview.instance, scope) {
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
    crate::camera_views_ui::after_fit(w);
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
        let pose = crate::camera_views_ui::current(&w);
        let (forward, right, up) = basis(&w);
        *t = Transform {
            translation: eye(&w),
            rotation: Quat::from_mat3(&Mat3::from_cols(right, up, -forward)),
            ..default()
        };
        let distance = (pose.distance_mm * 0.01) as f32;
        let near = (distance * 0.0001).max(0.00001);
        let far = (distance * 100.).max(100.);
        match pose.projection {
            cad_core::camera_views::Projection::Perspective { vertical_fov_deg } => {
                if !matches!(*projection, Projection::Perspective(_)) {
                    *projection = Projection::Perspective(PerspectiveProjection::default());
                }
                if let Projection::Perspective(p) = &mut *projection {
                    p.fov = vertical_fov_deg.to_radians() as f32;
                    p.near = near;
                    p.far = far;
                    p.aspect_ratio = w.viewport_size[0].clamp(32, 4096) as f32
                        / w.viewport_size[1].clamp(32, 4096) as f32;
                }
            }
            cad_core::camera_views::Projection::Orthographic { vertical_span_mm } => {
                if !matches!(*projection, Projection::Orthographic(_)) {
                    *projection = Projection::Orthographic(OrthographicProjection::default_3d());
                }
                if let Projection::Orthographic(p) = &mut *projection {
                    p.scaling_mode = bevy::camera::ScalingMode::FixedVertical {
                        viewport_height: (vertical_span_mm * 0.01) as f32,
                    };
                    p.scale = 1.;
                    p.near = near;
                    p.far = far;
                    bevy::camera::CameraProjection::update(
                        p,
                        w.viewport_size[0].clamp(32, 4096) as f32,
                        w.viewport_size[1].clamp(32, 4096) as f32,
                    );
                }
            }
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
    let selection_source = selection_evaluation(&w)
        .map(|evaluation| {
            serde_json::to_string(&(
                &evaluation.identity,
                display_instances(&w, evaluation)
                    .iter()
                    .map(|instance| (&instance.path, &instance.matrix))
                    .collect::<Vec<_>>(),
                &w.view.history_cursor,
            ))
            .unwrap_or_default()
        })
        .unwrap_or_default();
    if selection_source != w.selection_source {
        w.selection_source = selection_source;
        w.candidates.clear();
        w.selection_popup = None;
        w.selected = None;
        w.geometry_selection.clear();
        w.hover_guide = None;
        w.hover_cache = crate::viewport_guides::Cache::default();
        if matches!(
            w.context_selection,
            Some(cad_core::selection_actions::SelectionTarget::Geometry { .. })
        ) {
            w.context_selection = None;
        }
    }
    if w.pending_context_focus {
        if w.sketch.is_some() {
            w.pending_context_focus = false;
        } else if display_evaluation(&w).is_some_and(|e| e.identity == w.session.identity()) {
            fit_context(&mut w);
            w.pending_context_focus = false;
        }
    }
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
        (
            w.view.history_cursor.clone(),
            w.view.exploded_view.clone(),
            w.view.display_state.clone()
        ),
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
        "{key}:{}:{:?}:{:?}",
        w.display_mesh.epoch,
        display_evaluation(&w).map(|e| (&e.identity, e.valid)),
        w.geometry_selection
            .iter()
            .map(|s| (
                &s.definition,
                &s.path,
                &s.body,
                &s.reference.producer,
                &s.reference.role,
                &s.reference.topology
            ))
            .collect::<Vec<_>>()
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
        let mut configured_parts = BTreeMap::new();
        for instance in display_instances(&w, e) {
            if !visible(&w, instance) {
                continue;
            }
            let sources = display_meshes(&w, e, instance);
            let display_effect = crate::display_states_ui::effect(&w, &instance.path);
            let has_overrides = doc.parts.get(&instance.definition).is_some_and(|part| {
                !part.face_finishes.is_empty() || part.features.iter().any(|feature| matches!(
                    &feature.operation, cad_core::Operation::WeldBead { definition } if definition.finish.is_some()
                ))
            });
            let part = if has_overrides {
                configured_parts
                    .entry((instance.definition.clone(), instance.configuration.clone()))
                    .or_insert_with(|| {
                        cad_core::configuration::effective_part(
                            doc,
                            &instance.definition,
                            instance.configuration.as_deref(),
                        )
                        .ok()
                    })
                    .as_ref()
            } else {
                None
            };
            let face_finishes = part
                .map(|part| cad_core::appearance::resolve_face_finishes(part, &sources))
                .unwrap_or_default();
            for source in sources {
                for (batch, assigned_finish) in finish_batches(source, part, &face_finishes) {
                    let m = batch.as_ref();
                    if m.positions.is_empty() || m.indices.is_empty() {
                        continue;
                    }
                    let ghost_neighbor = ghost_neighbor(&w, instance);
                    let ghost = history || ghost_neighbor;
                    let mesh = meshes.add(render_mesh(m, instance));
                    let finish = assigned_finish
                        .as_ref()
                        .or(display_effect.finish.as_ref())
                        .or_else(|| resolved_finishes.get(&instance.path))
                        .or(instance.finish.as_ref())
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
                            tint: if ghost {
                                Some(base_color)
                            } else if crate::assembly_motion_ui::component_selected(
                                &w,
                                &instance.path,
                            ) {
                                Some(style_color(
                                    &props,
                                    "selection-color",
                                    Color::srgb(1., 0.38, 0.055),
                                ))
                            } else {
                                None
                            },
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
                        entity
                            .insert((bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver));
                    }
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
    let mut selected_faces = w
        .geometry_selection
        .iter()
        .filter(|s| s.reference.topology == "face")
        .collect::<Vec<_>>();
    if let Some(selection) = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "face")
        && !selected_faces.iter().any(|s| {
            s.path == selection.path
                && s.definition == selection.definition
                && selected_topology(s, &selection.reference, &selection.body)
        })
    {
        selected_faces.push(selection);
    }
    for selection in selected_faces {
        if let Some(evaluation) = selection_evaluation(&w)
            && let Some(instance) = display_instances(&w, evaluation)
                .into_iter()
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
                let normal_matrix = normal_matrix(instance);
                let normals = indices
                    .iter()
                    .map(|index| {
                        render_point(
                            (normal_matrix * DVec3::from_array(source.normals[*index as usize]))
                                .to_array(),
                        )
                        .normalize_or_zero()
                        .to_array()
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
}
fn weld_guides(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter().with_clip_rect(rect);
    for preview in crate::weld_beads_ui::current_joint_previews(w) {
        let matrix = if preview.path.is_empty()
            && w.session
                .visible()
                .parts
                .contains_key(&preview.owner.definition)
        {
            display_evaluation(w)
                .and_then(|e| {
                    display_instances(w, e).into_iter().find(|instance| {
                        instance.definition == preview.owner.definition
                            && w.edge_selection_path
                                .as_ref()
                                .is_none_or(|path| path == &instance.path)
                            && w.view
                                .active_instance
                                .as_ref()
                                .is_none_or(|id| instance.path.last() == Some(id))
                    })
                })
                .map(|instance| instance.matrix)
                .unwrap_or(preview.matrix)
        } else {
            preview.matrix
        };
        let transform = |point: [f64; 3]| {
            std::array::from_fn(|axis| {
                matrix[axis][0] * point[0]
                    + matrix[axis][1] * point[1]
                    + matrix[axis][2] * point[2]
                    + matrix[axis][3]
            })
        };
        let prefix = format!(
            "weld-joint:{}/{}/{}",
            preview.owner.definition,
            preview.owner.node,
            preview.path.join("/")
        );
        let mut labeled = std::collections::BTreeSet::new();
        for (index, guide) in preview.guides.iter().enumerate() {
            let state = match guide.valid {
                Some(true) => "ready",
                Some(false) => "invalid",
                None => "pending",
            };
            let classes = vec![
                "viewport-guide".into(),
                "weld-guide".into(),
                if guide.label == "Seam root" {
                    "weld-root".into()
                } else {
                    "weld-toe".into()
                },
            ];
            let props = w.theme.properties_for(&style_target(w).child(
                &format!("{prefix}/guide/{index}"),
                "weld-joint-guide",
                &classes,
                state,
            ));
            if props.get("display").is_some_and(|v| v == "none")
                || props.get("visibility").is_some_and(|v| v == "hidden")
            {
                continue;
            }
            let color = props
                .get("color")
                .and_then(|v| crate::ui::color(v))
                .unwrap_or_else(|| match guide.valid {
                    Some(false) => egui::Color32::from_rgb(205, 48, 36),
                    None => egui::Color32::from_rgb(190, 132, 35),
                    Some(true) => egui::Color32::from_rgb(30, 143, 158),
                });
            let width = crate::presentation_controls::number(
                &props,
                "stroke-width",
                if guide.valid == Some(false) { 3. } else { 1.5 },
            )
            .max(0.);
            for pair in guide.points.windows(2) {
                if let (Some(a), Some(b)) = (
                    project(w, rect, transform(pair[0])),
                    project(w, rect, transform(pair[1])),
                ) {
                    painter.line_segment([a, b], egui::Stroke::new(width, color));
                }
            }
            if guide.valid == Some(false)
                && labeled.insert(guide.support)
                && let Some(point) = guide
                    .points
                    .get(guide.points.len() / 2)
                    .and_then(|point| project(w, rect, transform(*point)))
            {
                painter.circle_stroke(
                    point,
                    crate::presentation_controls::number(&props, "handle-size", 5.).max(0.),
                    egui::Stroke::new(width, color),
                );
                painter.text(
                    point + egui::vec2(6., -6.),
                    egui::Align2::LEFT_BOTTOM,
                    format!(
                        "Support {} · {} · contact failed",
                        guide.support + 1,
                        guide.label
                    ),
                    egui::FontId::proportional(crate::presentation_controls::number(
                        &props,
                        "font-size",
                        11.,
                    )),
                    color,
                );
            }
        }
        let props = w.theme.properties_for(&style_target(w).child(
            &format!("{prefix}/seam"),
            "weld-joint-guide",
            &[
                "viewport-guide".into(),
                "weld-guide".into(),
                "weld-seam".into(),
            ],
            if preview.complete { "ready" } else { "invalid" },
        ));
        if props.get("display").is_some_and(|v| v == "none")
            || props.get("visibility").is_some_and(|v| v == "hidden")
        {
            continue;
        }
        let color = props
            .get("color")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or_else(|| selection_color(w));
        let width = crate::presentation_controls::number(&props, "stroke-width", 2.).max(0.);
        for pair in preview.seam_points.windows(2) {
            if let (Some(a), Some(b)) = (
                project(w, rect, transform(pair[0])),
                project(w, rect, transform(pair[1])),
            ) {
                painter.line_segment([a, b], egui::Stroke::new(width, color));
            }
        }
        if let (Some(orientation), Some(origin)) = (
            preview.orientation,
            preview.seam_points.get(preview.seam_points.len() / 2),
        ) {
            let size = cad_core::expression::value(
                w.session.visible(),
                &preview.owner.definition,
                &preview.source.size,
                cad_core::Dimension::Length,
            )
            .unwrap_or(2.);
            for (index, direction) in orientation.into_iter().enumerate() {
                let end = std::array::from_fn(|axis| origin[axis] + direction[axis] * size);
                if let (Some(a), Some(b)) = (
                    project(w, rect, transform(*origin)),
                    project(w, rect, transform(end)),
                ) {
                    painter.arrow(a, b - a, egui::Stroke::new(width, color));
                    painter.text(
                        b,
                        egui::Align2::LEFT_BOTTOM,
                        if preview.source.profile == cad_core::weld_beads::Profile::Butt {
                            if index == 0 {
                                "Bead width".into()
                            } else {
                                "Crown direction".into()
                            }
                        } else {
                            format!("Support {} direction", index + 1)
                        },
                        egui::FontId::proportional(crate::presentation_controls::number(
                            &props,
                            "font-size",
                            10.,
                        )),
                        color,
                    );
                }
            }
        }
    }
}
pub fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    weld_guides(w, ui, rect);
    crate::fabrication_sources_ui::overlay(w, ui, rect);
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
                }) || w.geometry_selection.iter().any(|pick| {
                    selected_instance(pick, i)
                        && (selected_topology(pick, &edge.reference, &edge.body)
                            || pick.reference.topology == "face"
                                && pick.body == edge.body
                                && edge.faces.iter().any(|face| {
                                    face.producer == pick.reference.producer
                                        && face.role == pick.reference.role
                                }))
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
            .into_iter()
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
        .into_iter()
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
