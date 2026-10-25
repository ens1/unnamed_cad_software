//! Zoom-dependent native display detail, isolated from modeling/Apply evaluation.
use crate::{Workspace, presentation_controls as controls, viewport, worker};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{PlacedPart, display_mesh::*, presentation::SemanticElement, protocol::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};
pub struct State {
    worker: worker::WorkerClient,
    desired: String,
    context: String,
    generation: u64,
    changed: Instant,
    pending: Option<(u64, RequestIdentity)>,
    pub output: Option<Output>,
    attempted: u64,
    pub epoch: u64,
    pub frame: u64,
    pub status: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            worker: worker::WorkerClient::new(),
            desired: String::new(),
            context: String::new(),
            generation: 0,
            changed: Instant::now(),
            pending: None,
            output: None,
            attempted: 0,
            epoch: 0,
            frame: 0,
            status: String::new(),
        }
    }
}
fn target(e: &Evaluation, i: &PlacedPart, cursor: Option<&cad_core::NodeRef>) -> Option<Target> {
    // Assembly-owned deposited metal has its own evaluated cache, rather than
    // a part feature graph. Do not issue a part rebuild for that placement.
    if !e.history.contains_key(cad_core::history::mesh_key(i)) {
        return None;
    }
    let meshes = cad_core::history::display_meshes(e, i, cursor);
    let outputs = meshes
        .iter()
        .flat_map(|m| m.faces.iter().filter_map(|f| f.body.as_ref()))
        .filter(|b| b.definition == i.definition)
        .map(|b| b.node.clone())
        .collect::<BTreeSet<_>>();
    if outputs.is_empty() {
        return None;
    }
    let outputs = outputs.into_iter().collect::<Vec<_>>();
    let key = serde_json::to_string(&(cad_core::history::mesh_key(i), &outputs)).ok()?;
    Some(Target {
        key,
        definition: i.definition.clone(),
        configuration: i.configuration.clone(),
        outputs,
    })
}
pub fn quality(w: &Workspace) -> Quality {
    let mut q = w.view.display_quality.clone();
    let props = viewport::style(w);
    if let Some(v) = props.get("renderer-display-adaptive") {
        q.automatic = v == "true";
    }
    for (key, value) in [
        ("renderer-display-chord-pixels", &mut q.chord_pixels),
        ("renderer-display-minimum-mm", &mut q.minimum_mm),
        ("renderer-display-maximum-mm", &mut q.maximum_mm),
    ] {
        if let Some(v) = props
            .get(key)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
        {
            *value = v;
        }
    }
    q.chord_pixels = q.chord_pixels.clamp(0.05, 8.);
    q.minimum_mm = q.minimum_mm.clamp(1e-6, 1.);
    q.maximum_mm = q.maximum_mm.clamp(q.minimum_mm, 1.);
    q
}
fn desired(w: &Workspace) -> Option<(RequestIdentity, Vec<Target>, f64)> {
    let e = viewport::display_evaluation(w)?;
    if !e.valid {
        return None;
    }
    let q = quality(w);
    if !q.automatic {
        return None;
    }
    let pose = crate::camera_views_ui::current(w);
    let frame = pose.frame();
    let eye = DVec3::from_array(frame.eye_mm);
    let forward = DVec3::from_array(frame.forward);
    let height = f64::from(w.viewport_size[1].clamp(32, 4096));
    let mut mm_pixel = pose.vertical_span_mm() / height;
    let mut targets = BTreeMap::new();
    for i in viewport::display_instances(w, e) {
        if !viewport::visible(w, i) {
            continue;
        }
        if let Some(target) = target(e, i, w.view.history_cursor.as_ref()) {
            targets.insert(target.key.clone(), target);
        }
        let norm_one = (0..3)
            .map(|column| (0..3).map(|row| i.matrix[row][column].abs()).sum::<f64>())
            .fold(0., f64::max);
        let norm_infinity = (0..3)
            .map(|row| {
                (0..3)
                    .map(|column| i.matrix[row][column].abs())
                    .sum::<f64>()
            })
            .fold(0., f64::max);
        let scale_bound = (norm_one * norm_infinity).sqrt().max(1e-9);
        let mut local_mm_pixel = pose.vertical_span_mm() / height / scale_bound;
        if matches!(
            pose.projection,
            cad_core::camera_views::Projection::Perspective { .. }
        ) {
            for m in cad_core::history::display_meshes(e, i, w.view.history_cursor.as_ref()) {
                let mut near = f64::INFINITY;
                for bits in 0..8 {
                    let p = std::array::from_fn(|axis| m.bounds[(bits >> axis) & 1][axis]);
                    let depth = (DVec3::from_array(viewport::placed(p, i)) - eye).dot(forward);
                    if depth > pose.distance_mm * 0.001 {
                        near = near.min(depth);
                    }
                }
                if near.is_finite() {
                    local_mm_pixel = local_mm_pixel.min(
                        near / pose.distance_mm * pose.vertical_span_mm() / height / scale_bound,
                    );
                }
            }
        }
        mm_pixel = mm_pixel.min(local_mm_pixel);
        if targets.len() >= MAX_TARGETS {
            break;
        }
    }
    // Quantized levels debounce small zoom/orbit motions. Refinement is bounded
    // independently from the saved modeling tolerance and physical finish scale.
    let raw = (q.chord_pixels * mm_pixel).clamp(q.minimum_mm, q.maximum_mm);
    let tolerance = 2f64
        .powf((raw.log2() * 2.).floor() / 2.)
        .clamp(q.minimum_mm, q.maximum_mm);
    Some((
        e.identity.clone(),
        targets.into_values().collect(),
        tolerance,
    ))
}
pub fn poll(w: &mut Workspace) {
    w.display_mesh.frame = w.display_mesh.frame.wrapping_add(1);
    let desired = desired(w);
    if let Some((identity, targets, tolerance)) = &desired {
        let context = format!("{identity:?}:{}", w.root().display());
        if context != w.display_mesh.context {
            // Relative resources and native caches belong to this exact source
            // and project root. A stale job must not block the next document.
            w.display_mesh.worker.cancel();
            w.display_mesh.pending = None;
            w.display_mesh.output = None;
            w.display_mesh.status.clear();
            w.display_mesh.context = context;
            w.display_mesh.epoch = w.display_mesh.epoch.wrapping_add(1);
            w.rendered_key.clear();
        }
        let key = format!("{}:{targets:?}:{tolerance}", w.display_mesh.context);
        if key != w.display_mesh.desired {
            w.display_mesh.desired = key;
            w.display_mesh.generation = w.display_mesh.generation.wrapping_add(1);
            w.display_mesh.changed = Instant::now();
        }
    } else if !w.display_mesh.desired.is_empty() {
        w.display_mesh.worker.cancel();
        w.display_mesh.pending = None;
        w.display_mesh.output = None;
        w.display_mesh.desired.clear();
        w.display_mesh.context.clear();
        w.display_mesh.status.clear();
        w.display_mesh.generation = w.display_mesh.generation.wrapping_add(1);
        w.display_mesh.epoch = w.display_mesh.epoch.wrapping_add(1);
        w.rendered_key.clear();
    }
    for response in w.display_mesh.worker.drain() {
        match response {
            WorkerResponse::DisplayMesh { output } => {
                if w.display_mesh.pending.as_ref()
                    == Some(&(output.generation, output.identity.clone()))
                {
                    w.display_mesh.pending = None;
                    if output.version == VERSION && output.generation == w.display_mesh.generation {
                        w.display_mesh.status = output
                            .diagnostics
                            .iter()
                            .map(|d| d.message.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ");
                        w.display_mesh.output = Some(*output);
                        w.display_mesh.epoch = w.display_mesh.epoch.wrapping_add(1);
                        w.rendered_key.clear();
                        w.hover_cache = Default::default();
                    }
                }
            }
            WorkerResponse::Error { identity, message }
                if w.display_mesh
                    .pending
                    .as_ref()
                    .is_some_and(|(_, i)| Some(i) == identity.as_ref()) =>
            {
                w.display_mesh.pending = None;
                w.display_mesh.status = format!("Original display detail retained · {message}");
            }
            _ => {}
        }
    }
    let Some((identity, targets, tolerance)) = desired else {
        return;
    };
    // Zoom-only changes wait for the bounded current job, then request the
    // latest level. This preserves its native cache without accepting stale
    // tessellation into the viewport or the modeling session.
    if w.display_mesh.pending.is_some() || w.display_mesh.attempted == w.display_mesh.generation {
        return;
    }
    let debounce = std::time::Duration::from_millis(180);
    if w.display_mesh.changed.elapsed() < debounce {
        crate::redraw::request_after(debounce.saturating_sub(w.display_mesh.changed.elapsed()));
        return;
    }
    if w.display_mesh
        .output
        .as_ref()
        .is_some_and(|o| o.generation == w.display_mesh.generation)
    {
        return;
    }
    let document = if w.session.committed.fingerprint() == identity.source {
        &w.session.committed
    } else if w.session.visible().fingerprint() == identity.source {
        w.session.visible()
    } else {
        return;
    };
    if targets.is_empty() {
        return;
    }
    let request = Request {
        version: VERSION,
        identity: identity.clone(),
        generation: w.display_mesh.generation,
        document: document.clone(),
        project_root: w.root().to_string_lossy().into(),
        modeling: document.settings.modeling,
        display_tolerance_mm: tolerance,
        targets,
    };
    w.display_mesh.attempted = request.generation;
    w.display_mesh.pending = Some((request.generation, identity));
    w.display_mesh.status = "Refining viewport detail · current display retained".into();
    w.display_mesh.worker.submit(WorkerRequest::DisplayMesh {
        request: Box::new(request),
    });
}
pub fn replacement<'a>(w: &'a Workspace, e: &Evaluation, i: &PlacedPart) -> Option<&'a MeshData> {
    if !quality(w).automatic {
        return None;
    }
    let output = w
        .display_mesh
        .output
        .as_ref()
        .filter(|o| o.version == VERSION && o.identity == e.identity)?;
    output
        .meshes
        .get(&target(e, i, w.view.history_cursor.as_ref())?.key)
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    match command {
        "viewport.detail-adaptive" => {
            w.view.display_quality.automatic = !w.view.display_quality.automatic
        }
        "viewport.detail-fine" => w.view.display_quality.chord_pixels = 0.25,
        "viewport.detail-balanced" => w.view.display_quality.chord_pixels = 0.5,
        "viewport.detail-coarse" => w.view.display_quality.chord_pixels = 1.,
        _ => return false,
    }
    w.rendered_key.clear();
    true
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let q = quality(w);
    vec![SemanticElement {
        id: "workspace.viewport-detail".into(),
        role: "viewport-detail".into(),
        label: "Display detail".into(),
        value: Some(format!(
            "{} px chord · {}",
            q.chord_pixels, w.display_mesh.status
        )),
        state: if w.display_mesh.pending.is_some() {
            "pending"
        } else {
            "ready"
        }
        .into(),
        commands: vec![
            "viewport.detail-adaptive".into(),
            "viewport.detail-fine".into(),
            "viewport.detail-balanced".into(),
            "viewport.detail-coarse".into(),
        ],
        relationships: vec!["workspace.viewport".into()],
        data: serde_json::json!({"contract":"tcad.display-mesh", "version":VERSION,
            "geometry_changes":false,"apply_eligibility_changes":false,"quality":q,"diagnostic":w.display_mesh.status,
            "theme_properties":["renderer-display-adaptive","renderer-display-chord-pixels","renderer-display-minimum-mm","renderer-display-maximum-mm"]}),
    }]
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.menu_button("Display detail", |ui| {
        let q = quality(w);
        for (id, label, selected) in [("viewport.detail-adaptive", "Refine as I zoom", q.automatic),
            ("viewport.detail-fine", "Fine · 0.25 px", q.chord_pixels == 0.25),
            ("viewport.detail-balanced", "Balanced · 0.5 px", q.chord_pixels == 0.5),
            ("viewport.detail-coarse", "Economy · 1 px", q.chord_pixels == 1.)] {
            if controls::button(ui, id, "viewport-detail-action", &["menu-action"], label, true, selected).clicked() {
                action(w, id); ui.close();
            }
        }
        ui.small("View detail changes independently from modeling accuracy. Native curves and finish scale stay unchanged.");
        if !w.display_mesh.status.is_empty() { ui.small(&w.display_mesh.status); }
    });
}
