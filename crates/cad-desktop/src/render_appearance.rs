//! Replaceable Bevy adapter for application-owned visual finishes and studio lighting.
//! Texture coordinates are part-local millimetres, independent of CAD tessellation UVs.
use crate::{CadCamera, Workspace};
use bevy::{
    anti_alias::fxaa::Fxaa,
    asset::{RenderAssetUsages, uuid_handle},
    camera::{Exposure, Hdr, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::CascadeShadowConfigBuilder,
    math::DMat4,
    pbr::{
        ExtendedMaterial, MaterialExtension, ScreenSpaceAmbientOcclusion,
        ScreenSpaceAmbientOcclusionQualityLevel,
    },
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, Face, ShaderType, TextureDimension, TextureFormat,
    },
    shader::{Shader, ShaderRef},
};
use cad_core::{
    Document, PlacedPart,
    appearance::{BuiltinBump, Finish, MappingMode},
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
};

const FINISH_SHADER: Handle<Shader> = uuid_handle!("c201dc9b-78b5-40d4-a148-a153ba4f04d4");
const CACHE_BYTES: usize = 512 * 1024 * 1024;
const CACHE_ITEMS: usize = 128;
const PARALLEL_LOADS: usize = 2;
// The surrounding studio supplies most of a metal's light. Small direct lights
// retain readable contact shadows without washing the finish into flat white.
const STUDIO_INTENSITY: f32 = 1300.;
const KEY_ILLUMINANCE: f32 = 3000.;
const FILL_ILLUMINANCE: f32 = 1500.;
const KEY_DIRECTION: Vec3 = Vec3::new(-3., 7., 5.);
const FILL_DIRECTION: Vec3 = Vec3::new(4., 3., -4.);
pub type FinishMaterial = ExtendedMaterial<StandardMaterial, FinishExtension>;

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct FinishUniform {
    world_to_part: Mat4,
    part_to_world: Mat4,
    mapping: Vec4,
    settings: Vec4,
    relief: Vec4,
    surface: Vec4,
}
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FinishExtension {
    #[uniform(100)]
    #[reflect(ignore)]
    uniform: FinishUniform,
    #[texture(101)]
    #[sampler(102)]
    color: Handle<Image>,
    #[texture(103)]
    normal: Handle<Image>,
    #[texture(105)]
    roughness: Handle<Image>,
    #[texture(107)]
    metallic: Handle<Image>,
    #[texture(109)]
    occlusion: Handle<Image>,
    #[texture(111)]
    height: Handle<Image>,
}
impl MaterialExtension for FinishExtension {
    fn fragment_shader() -> ShaderRef {
        FINISH_SHADER.into()
    }
}
pub struct FinishRenderPlugin;
impl Plugin for FinishRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FinishMaterial>::default())
            .init_resource::<Runtime>();
        let shader = Shader::from_wesl(
            include_str!("finish_material.wesl"),
            "embedded://tcad/finish_material.wesl",
        );
        // A UUID asset has no generation slot and cannot fail insertion.
        if let Err(error) = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .insert(FINISH_SHADER.id(), shader)
        {
            bevy::log::error!("Cannot register the embedded finish shader: {error}");
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct TextureKey {
    root: PathBuf,
    path: String,
    sha256: String,
    srgb: bool,
    height: bool,
}
enum TextureState {
    Pending,
    Loaded(Handle<Image>),
    Failed(String),
}
struct LoadRequest {
    key: TextureKey,
    resource: cad_core::Resource,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EnvironmentKey {
    root: PathBuf,
    path: String,
    sha256: String,
}
struct EnvironmentLoaded {
    diffuse: Handle<Image>,
    specular: Handle<Image>,
    compensation: f32,
    normalization: f32,
    dimensions: [u32; 2],
    maximum: f32,
    bytes: usize,
    last_used: u64,
}
enum EnvironmentState {
    Pending,
    Loaded(EnvironmentLoaded),
    Failed(String),
}
fn environment_key(doc: &Document, root: &Path) -> Option<EnvironmentKey> {
    let resource = doc.resources.get(doc.lighting.environment.as_ref()?)?;
    Some(EnvironmentKey {
        root: root.to_path_buf(),
        path: resource.path.clone(),
        sha256: resource.sha256.clone(),
    })
}
enum Reply {
    Texture(TextureKey, Result<Image, String>),
    BuiltinHeight(u64, BuiltinBump, Result<Image, String>),
    Studio(Image, Image),
    Environment(
        EnvironmentKey,
        Result<crate::environment_filter::Filtered, String>,
    ),
}
#[derive(Resource)]
pub struct Runtime {
    textures: HashMap<TextureKey, TextureState>,
    queue: VecDeque<LoadRequest>,
    sender: mpsc::Sender<Reply>,
    receiver: Mutex<mpsc::Receiver<Reply>>,
    loading: usize,
    image_bytes: usize,
    white: Handle<Image>,
    flat_normal: Handle<Image>,
    builtin_height: BTreeMap<BuiltinBump, TextureState>,
    builtin_generation: u64,
    pub epoch: u64,
    studio: Option<(Handle<Image>, Handle<Image>)>,
    usage_key: String,
    environments: BTreeMap<EnvironmentKey, EnvironmentState>,
    environment_loading: Option<EnvironmentKey>,
    desired_environment: Option<EnvironmentKey>,
    last_valid_environment: Option<EnvironmentKey>,
    environment_clock: u64,
}
impl Default for Runtime {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            textures: HashMap::new(),
            queue: VecDeque::new(),
            sender,
            receiver: Mutex::new(receiver),
            loading: 0,
            image_bytes: 0,
            white: Handle::default(),
            flat_normal: Handle::default(),
            builtin_height: BTreeMap::new(),
            builtin_generation: u64::MAX,
            epoch: 0,
            studio: None,
            usage_key: String::new(),
            environments: BTreeMap::new(),
            environment_loading: None,
            desired_environment: None,
            last_valid_environment: None,
            environment_clock: 0,
        }
    }
}
impl Runtime {
    fn texture(
        &mut self,
        doc: &Document,
        root: &Path,
        id: Option<&str>,
        srgb: bool,
        height: bool,
    ) -> Option<Handle<Image>> {
        let resource = doc.resources.get(id?)?;
        let key = TextureKey {
            root: root.to_path_buf(),
            path: resource.path.clone(),
            sha256: resource.sha256.clone(),
            srgb,
            height,
        };
        if let Some(state) = self.textures.get(&key) {
            return match state {
                TextureState::Loaded(handle) => Some(handle.clone()),
                _ => None,
            };
        }
        if self.textures.len() >= CACHE_ITEMS {
            return None;
        }
        self.textures.insert(key.clone(), TextureState::Pending);
        self.queue.push_back(LoadRequest {
            key,
            resource: resource.clone(),
        });
        None
    }
    fn retain_sources(&mut self, documents: &[&Document], root: &Path, images: &mut Assets<Image>) {
        let mut used = std::collections::HashSet::new();
        for doc in documents {
            for finish in doc.finishes.values() {
                for (slot, (id, srgb)) in [
                    (&finish.textures.base_color, true),
                    (&finish.textures.normal, false),
                    (&finish.textures.roughness, false),
                    (&finish.textures.metallic, false),
                    (&finish.textures.occlusion, false),
                    (&finish.textures.height, false),
                ]
                .into_iter()
                .enumerate()
                {
                    let Some(resource) = id.as_ref().and_then(|id| doc.resources.get(id)) else {
                        continue;
                    };
                    used.insert(TextureKey {
                        root: root.to_path_buf(),
                        path: resource.path.clone(),
                        sha256: resource.sha256.clone(),
                        srgb,
                        height: slot == 5,
                    });
                }
            }
        }
        self.queue.retain(|request| used.contains(&request.key));
        let obsolete = self
            .textures
            .keys()
            .filter(|key| !used.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in obsolete {
            if let Some(TextureState::Loaded(handle)) = self.textures.remove(&key)
                && let Some(image) = images.remove(&handle)
            {
                self.image_bytes = self
                    .image_bytes
                    .saturating_sub(image.data.as_ref().map(Vec::len).unwrap_or(0));
            }
        }
        // Retry failed resources once per source change, including restored files and reimports.
        self.textures
            .retain(|_, state| !matches!(state, TextureState::Failed(_)));
        self.environments
            .retain(|_, state| !matches!(state, EnvironmentState::Failed(_)));
        self.epoch = self.epoch.wrapping_add(1);
    }
    fn select_environment(&mut self, doc: &Document, root: &Path) {
        let wanted = environment_key(doc, root);
        if self.desired_environment != wanted {
            self.desired_environment = wanted.clone();
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.environment_clock = self.environment_clock.wrapping_add(1);
        self.environments.retain(|key, state| {
            !matches!(state, EnvironmentState::Pending)
                || Some(key) == wanted.as_ref()
                || Some(key) == self.environment_loading.as_ref()
        });
        if let Some(key) = wanted {
            let state = self
                .environments
                .entry(key.clone())
                .or_insert(EnvironmentState::Pending);
            if let EnvironmentState::Loaded(loaded) = state {
                loaded.last_used = self.environment_clock;
                self.last_valid_environment = Some(key);
            }
        }
    }
    fn environment_maps(&self) -> Option<(&Handle<Image>, &Handle<Image>, f32)> {
        if self.desired_environment.is_some() {
            for key in [&self.desired_environment, &self.last_valid_environment]
                .into_iter()
                .flatten()
            {
                if let Some(EnvironmentState::Loaded(loaded)) = self.environments.get(key) {
                    return Some((&loaded.diffuse, &loaded.specular, loaded.compensation));
                }
            }
        }
        self.studio
            .as_ref()
            .map(|(diffuse, specular)| (diffuse, specular, 1.))
    }
    fn trim_environments(&mut self, images: &mut Assets<Image>) {
        // Keep a previous fully validated map for pending previews and quick Cancel/undo.
        let mut loaded = self
            .environments
            .iter()
            .filter_map(|(key, state)| match state {
                EnvironmentState::Loaded(loaded) => Some((loaded.last_used, key.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        loaded.sort();
        let obsolete = loaded.len().saturating_sub(2);
        for (_, key) in loaded.into_iter().take(obsolete) {
            if self.desired_environment.as_ref() == Some(&key)
                || self.last_valid_environment.as_ref() == Some(&key)
            {
                continue;
            }
            if let Some(EnvironmentState::Loaded(loaded)) = self.environments.remove(&key) {
                images.remove(&loaded.diffuse);
                images.remove(&loaded.specular);
                self.image_bytes = self.image_bytes.saturating_sub(loaded.bytes);
            }
        }
        self.environments.retain(|key, state| {
            !matches!(state, EnvironmentState::Failed(_))
                || Some(key) == self.desired_environment.as_ref()
        });
    }
    fn environment_info(&self, doc: &Document, root: &Path) -> serde_json::Value {
        let key = environment_key(doc, root);
        let state = key.as_ref().and_then(|key| self.environments.get(key));
        let mut info = serde_json::json!({"contract":"tcad.environment-render","version":1,"resource":doc.lighting.environment,"state":if key.is_none(){if doc.lighting.environment.is_some(){"failed"}else{"studio"}}else{match state{Some(EnvironmentState::Loaded(_))=>"ready",Some(EnvironmentState::Failed(_))=>"failed",_=>"pending"}},"projection":"equirectangular-2-to-1","color_space":"linear-rgb","diffuse_cube_size":32,"specular_cube_size":128,"specular_mips":8,"filter_samples":256,"background":"workspace","affects_engineering_properties":false,"ray_tracing":false,"preview_fallback":doc.lighting.environment.is_some() && !matches!(state,Some(EnvironmentState::Loaded(_)))});
        if let Some(key) = &key {
            info["path"] = serde_json::json!(key.path);
            info["sha256"] = serde_json::json!(key.sha256);
        }
        if let Some(EnvironmentState::Failed(error)) = state {
            info["error"] = serde_json::json!(error);
        } else if doc.lighting.environment.is_some() && key.is_none() {
            info["error"] = "The selected environment resource is missing".into();
        }
        if let Some(EnvironmentState::Loaded(loaded)) = state {
            info["source_dimensions"] = serde_json::json!(loaded.dimensions);
            info["maximum_source_radiance"] = serde_json::json!(loaded.maximum);
            info["gpu_radiance_normalization"] = serde_json::json!(loaded.normalization);
            info["intensity_compensation"] = serde_json::json!(loaded.compensation);
        }
        info
    }
    /// Optional legacy maps never hold up a modeling transaction. Keep this
    /// distinct from diagnostics for checksum-pinned project resources, which
    /// must still validate before an appearance can be applied.
    fn repository_map_info(&self, doc: &Document) -> serde_json::Value {
        let kinds = doc
            .finishes
            .values()
            .filter(|finish| finish.textures.height.is_none())
            .map(|finish| finish.builtin_bump)
            .filter(|kind| *kind != BuiltinBump::None)
            .collect::<std::collections::BTreeSet<_>>();
        let maps = kinds.into_iter().filter_map(|kind| {
            let (state, detail) = match self.builtin_height.get(&kind) {
                Some(TextureState::Loaded(_)) => return None,
                Some(TextureState::Failed(error)) => ("unavailable", Some(error.as_str())),
                Some(TextureState::Pending) => ("loading", None),
                None => ("checking", None),
            };
            Some(serde_json::json!({"id":kind.repository_entry(),"name":kind.label(),"state":state,"detail":detail}))
        }).collect::<Vec<_>>();
        serde_json::json!({"pack":"textures","fallback":"smooth-finish","blocks_modeling":false,"maps":maps})
    }

    pub fn diagnostics(&self, doc: &Document, root: &Path, validating: bool) -> Vec<String> {
        let mut messages = vec![];
        let mut used = std::collections::BTreeSet::new();
        for finish in doc.finishes.values() {
            for (slot, (id, srgb)) in [
                (&finish.textures.base_color, true),
                (&finish.textures.normal, false),
                (&finish.textures.roughness, false),
                (&finish.textures.metallic, false),
                (&finish.textures.occlusion, false),
                (&finish.textures.height, false),
            ]
            .into_iter()
            .enumerate()
            {
                let Some(id) = id else { continue };
                let Some(resource) = doc.resources.get(id) else {
                    messages.push(format!(
                        "Finish {}: texture resource {id} is missing",
                        finish.name
                    ));
                    continue;
                };
                if !used.insert((id, srgb, slot == 5)) {
                    continue;
                }
                let key = TextureKey {
                    root: root.to_path_buf(),
                    path: resource.path.clone(),
                    sha256: resource.sha256.clone(),
                    srgb,
                    height: slot == 5,
                };
                match self.textures.get(&key) {
                    Some(TextureState::Failed(error)) => messages.push(format!("Texture {}: {error}", resource.path)),
                    Some(TextureState::Pending) => messages.push(format!("Loading texture {}", resource.path)),
                    None if self.textures.len() >= CACHE_ITEMS => messages.push(format!("Texture {}: renderer image cache limit reached ({CACHE_ITEMS} images); remove unused texture maps or reduce image sizes", resource.path)),
                    None if validating => messages.push(format!("Loading texture {}", resource.path)),
                    _ => {},
                }
            }
        }
        if let Some(resource) = doc
            .lighting
            .environment
            .as_ref()
            .and_then(|id| doc.resources.get(id))
        {
            let key = EnvironmentKey {
                root: root.to_path_buf(),
                path: resource.path.clone(),
                sha256: resource.sha256.clone(),
            };
            match self.environments.get(&key) {
                Some(EnvironmentState::Loaded(_)) => {},
                Some(EnvironmentState::Failed(error)) => messages.push(format!("Environment {}: {error}; the preview retains the previous validated environment or built-in studio",resource.path)),
                _ => messages.push(format!("Loading texture environment {} (decode and reflection filtering); the preview retains the previous validated environment or built-in studio",resource.path)),
            }
        } else if doc.lighting.environment.is_some() {
            messages.push("The selected environment resource is missing".into());
        }
        messages
    }
}

#[derive(Component)]
pub struct StudioLight(pub bool);

pub fn setup(commands: &mut Commands, images: &mut Assets<Image>, runtime: &mut Runtime) {
    runtime.white = images.add(pixel([255, 255, 255, 255]));
    runtime.flat_normal = images.add(pixel([128, 128, 255, 255]));
    // A coarse studio is available immediately. Filtering the final resources
    // stays off the main thread, but metal never starts in an unlit void.
    runtime.studio = Some((
        images.add(crate::environment_filter::cube_image(
            4,
            1,
            true,
            &studio_radiance,
        )),
        images.add(crate::environment_filter::cube_image(
            8,
            4,
            false,
            &studio_radiance,
        )),
    ));
    let sender = runtime.sender.clone();
    std::thread::spawn(move || {
        let (diffuse, specular) = studio_maps();
        if sender.send(Reply::Studio(diffuse, specular)).is_ok() {
            crate::redraw::wake();
        }
    });
    commands.insert_resource(GlobalAmbientLight {
        brightness: 0.0,
        ..default()
    });
    for (key, position, illuminance) in [
        (true, KEY_DIRECTION, KEY_ILLUMINANCE),
        (false, FILL_DIRECTION, FILL_ILLUMINANCE),
    ] {
        commands.spawn((
            DirectionalLight {
                illuminance,
                shadow_maps_enabled: key,
                shadow_depth_bias: 0.005,
                shadow_normal_bias: 0.08,
                ..default()
            },
            Transform::from_translation(position).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(1),
            StudioLight(key),
        ));
    }
}

pub fn camera_components() -> impl Bundle {
    (
        Hdr,
        Exposure { ev100: 10.0 },
        Tonemapping::AcesFitted,
        Msaa::Off,
        Fxaa::default(),
        ScreenSpaceAmbientOcclusion {
            quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
            radius: 0.15,
            constant_object_thickness: 0.01,
        },
    )
}

fn validation_key(w: &Workspace) -> String {
    format!(
        "{}:{}:{}:{}:{:?}:{:?}:{}",
        w.session.source_epoch(),
        w.session.generation,
        w.session.committed.revision,
        w.saved_source,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit)),
        w.root(),
        w.fields.contains_key("appearance-tool") || w.fields.contains_key("display-state-editor")
    )
}
pub(crate) fn resources_are_current(w: &Workspace) -> bool {
    w.appearance_validation_key == validation_key(w)
}
pub(crate) fn resources_pending(w: &Workspace) -> bool {
    !resources_are_current(w)
        || !w.appearance_diagnostics.is_empty()
            && w.appearance_diagnostics
                .iter()
                .all(|message| message.starts_with("Loading texture "))
}

pub fn poll_assets(
    mut runtime: ResMut<Runtime>,
    mut images: ResMut<Assets<Image>>,
    mut w: ResMut<Workspace>,
) {
    let root = w.root();
    let usage_key = format!(
        "{}:{}:{}:{}:{:?}:{}:{:?}",
        w.session.source_epoch(),
        w.session.generation,
        w.session.committed.revision,
        w.saved_source,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit)),
        w.compare
            .as_ref()
            .map(|evaluation| evaluation.identity.source.as_str())
            .unwrap_or(""),
        root
    );
    if runtime.usage_key != usage_key {
        runtime.usage_key = usage_key;
        let mut sources = vec![&w.session.committed];
        if let Some(draft) = &w.session.draft {
            sources.push(&draft.document);
        }
        if let Some(comparison) = &w.compare_document {
            sources.push(comparison);
        }
        runtime.retain_sources(&sources, &root, &mut images);
    }
    // Repository maps are only a compatibility path for old documents. New
    // assignments use checksum-pinned project resources. Refresh discards old
    // generations and refuses late worker results from an updated checkout.
    let generation = cad_core::content_repositories::catalog_generation();
    if runtime.builtin_generation != generation {
        let old = std::mem::take(&mut runtime.builtin_height);
        for state in old.into_values() {
            if let TextureState::Loaded(handle) = state
                && let Some(image) = images.remove(&handle)
            {
                runtime.image_bytes = runtime
                    .image_bytes
                    .saturating_sub(image.data.as_ref().map(Vec::len).unwrap_or(0));
            }
        }
        runtime.builtin_generation = generation;
        runtime.epoch = runtime.epoch.wrapping_add(1);
    }
    let wanted = w
        .session
        .visible()
        .finishes
        .values()
        .filter(|finish| finish.textures.height.is_none())
        .map(|finish| finish.builtin_bump)
        .filter(|kind| *kind != BuiltinBump::None)
        .collect::<std::collections::BTreeSet<_>>();
    for kind in wanted {
        if generation == 0 || runtime.builtin_height.contains_key(&kind) {
            continue;
        }
        runtime.builtin_height.insert(kind, TextureState::Pending);
        let sender = runtime.sender.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| crate::texture_assets::builtin_height(kind))
                .unwrap_or_else(|_| Err("Repository texture decoder failed".into()));
            if sender
                .send(Reply::BuiltinHeight(generation, kind, result))
                .is_ok()
            {
                crate::redraw::wake();
            }
        });
    }
    runtime.select_environment(w.session.visible(), &root);
    let validating =
        w.fields.contains_key("appearance-tool") || w.fields.contains_key("display-state-editor");
    let finishes = if validating {
        w.session.visible().finishes.values().collect::<Vec<_>>()
    } else {
        w.fields
            .get("appearance-editor")
            .and_then(|id| w.session.visible().finishes.get(id))
            .into_iter()
            .collect()
    };
    for finish in finishes {
        // Active transactions validate all referenced library images, including unassigned finishes.
        for (slot, (id, srgb)) in [
            (&finish.textures.base_color, true),
            (&finish.textures.normal, false),
            (&finish.textures.roughness, false),
            (&finish.textures.metallic, false),
            (&finish.textures.occlusion, false),
            (&finish.textures.height, false),
        ]
        .into_iter()
        .enumerate()
        {
            runtime.texture(w.session.visible(), &root, id.as_deref(), srgb, slot == 5);
        }
    }
    let replies = runtime
        .receiver
        .lock()
        .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for reply in replies {
        match reply {
            Reply::Texture(key, result) => {
                runtime.loading = runtime.loading.saturating_sub(1);
                if !matches!(runtime.textures.get(&key), Some(TextureState::Pending)) {
                    continue;
                }
                runtime.queue.retain(|request| request.key != key);
                let state = match result {
                    Ok(image) => {
                        let bytes = image.data.as_ref().map(Vec::len).unwrap_or(0);
                        if runtime.image_bytes.saturating_add(bytes) > CACHE_BYTES {
                            TextureState::Failed("Renderer texture memory limit (512 MiB) reached; remove unused texture maps or reduce image sizes".into())
                        } else {
                            runtime.image_bytes += bytes;
                            TextureState::Loaded(images.add(image))
                        }
                    }
                    Err(error) => TextureState::Failed(error),
                };
                runtime.textures.insert(key, state);
                runtime.epoch = runtime.epoch.wrapping_add(1);
            }
            Reply::BuiltinHeight(generation, kind, result) => {
                if generation != runtime.builtin_generation {
                    continue;
                }
                let state = match result {
                    Ok(image) => {
                        let bytes = image.data.as_ref().map(Vec::len).unwrap_or(0);
                        if runtime.image_bytes.saturating_add(bytes) > CACHE_BYTES {
                            TextureState::Failed("Renderer texture memory limit reached; pin only the maps used by this model".into())
                        } else {
                            runtime.image_bytes += bytes;
                            TextureState::Loaded(images.add(image))
                        }
                    }
                    Err(error) => TextureState::Failed(error),
                };
                runtime.builtin_height.insert(kind, state);
                runtime.epoch = runtime.epoch.wrapping_add(1);
            }
            Reply::Environment(key, result) => {
                if runtime.environment_loading.as_ref() == Some(&key) {
                    runtime.environment_loading = None;
                }
                if runtime.desired_environment.as_ref() != Some(&key) {
                    runtime.environments.remove(&key);
                    continue;
                }
                runtime.trim_environments(&mut images);
                let state = match result {
                    Ok(filtered) => {
                        let bytes = [&filtered.diffuse, &filtered.specular]
                            .into_iter()
                            .map(|image| image.data.as_ref().map(Vec::len).unwrap_or(0))
                            .sum::<usize>();
                        if runtime.image_bytes.saturating_add(bytes) > CACHE_BYTES {
                            EnvironmentState::Failed("Renderer texture memory limit (512 MiB) reached; remove unused maps or reduce image sizes".into())
                        } else {
                            runtime.image_bytes += bytes;
                            let loaded = EnvironmentLoaded {
                                diffuse: images.add(filtered.diffuse),
                                specular: images.add(filtered.specular),
                                compensation: filtered.intensity_compensation,
                                normalization: filtered.normalization,
                                dimensions: filtered.source_dimensions,
                                maximum: filtered.maximum_radiance,
                                bytes,
                                last_used: runtime.environment_clock,
                            };
                            runtime.last_valid_environment = Some(key.clone());
                            EnvironmentState::Loaded(loaded)
                        }
                    }
                    Err(error) => EnvironmentState::Failed(error),
                };
                runtime.environments.insert(key, state);
                runtime.trim_environments(&mut images);
                runtime.epoch = runtime.epoch.wrapping_add(1);
            }
            Reply::Studio(diffuse, specular) => {
                if let Some((diffuse, specular)) = runtime.studio.take() {
                    images.remove(&diffuse);
                    images.remove(&specular);
                }
                runtime.studio = Some((images.add(diffuse), images.add(specular)));
                runtime.epoch = runtime.epoch.wrapping_add(1);
            }
        }
    }
    while runtime.loading < PARALLEL_LOADS {
        let Some(request) = runtime.queue.pop_front() else {
            break;
        };
        let sender = runtime.sender.clone();
        runtime.loading += 1;
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                if request.key.height {
                    crate::texture_assets::load_height(&request.key.root, &request.resource)
                } else {
                    crate::texture_assets::load(
                        &request.key.root,
                        &request.resource,
                        request.key.srgb,
                    )
                }
            })
            .unwrap_or_else(|_| Err("Image decoder rejected the texture unexpectedly".into()));
            if sender.send(Reply::Texture(request.key, result)).is_ok() {
                crate::redraw::wake();
            }
        });
    }
    if runtime.environment_loading.is_none()
        && let Some(key) = runtime.desired_environment.clone()
        && matches!(
            runtime.environments.get(&key),
            Some(EnvironmentState::Pending)
        )
        && let Some(resource) = w
            .session
            .visible()
            .lighting
            .environment
            .as_ref()
            .and_then(|id| w.session.visible().resources.get(id))
            .cloned()
    {
        let sender = runtime.sender.clone();
        runtime.environment_loading = Some(key.clone());
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                crate::environment_assets::load(&key.root, &resource)
                    .map(|panorama| crate::environment_filter::panorama(&panorama))
            })
            .unwrap_or_else(|_| {
                Err(
                    "Environment decoder or reflection filter rejected this file unexpectedly"
                        .into(),
                )
            });
            if sender.send(Reply::Environment(key, result)).is_ok() {
                crate::redraw::wake();
            }
        });
    }
    let info = runtime.environment_info(w.session.visible(), &root);
    w.fields.insert(
        "appearance-environment-render-info".into(),
        info.to_string(),
    );
    let repository_maps = runtime.repository_map_info(w.session.visible());
    w.fields.insert(
        "appearance-repository-map-info".into(),
        repository_maps.to_string(),
    );
    w.appearance_diagnostics = runtime.diagnostics(w.session.visible(), &w.root(), validating);
    w.appearance_validation_key = validation_key(&w);
    if validating && w.confirm_requested && resources_are_current(&w) {
        if w.appearance_diagnostics.is_empty() {
            // Resource completion may follow geometry completion, without another worker reply.
            w.confirm_tool();
        } else if !resources_pending(&w) {
            w.confirm_requested = false;
            w.status =
                "Resolve the current texture or environment resource errors before applying".into();
        }
    }
}

fn style_number(
    props: &BTreeMap<String, String>,
    key: &str,
    fallback: f32,
    bounds: [f32; 2],
) -> f32 {
    props
        .get(key)
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
        .clamp(bounds[0], bounds[1])
}

pub fn lighting(
    mut commands: Commands,
    w: Res<Workspace>,
    runtime: Res<Runtime>,
    mut camera: Query<
        (
            Entity,
            &mut Exposure,
            Option<&mut ScreenSpaceAmbientOcclusion>,
        ),
        With<CadCamera>,
    >,
    mut lights: Query<(Entity, &mut DirectionalLight, &mut Transform, &StudioLight)>,
) {
    let settings = &w.session.visible().lighting;
    let props = crate::viewport::style(&w);
    let environment = style_number(
        &props,
        "environment-intensity",
        settings.intensity as f32,
        [0., 20.],
    );
    let rotation = props
        .get("environment-rotation")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(settings.environment_rotation_deg)
        .clamp(-1_000_000., 1_000_000.)
        .rem_euclid(360.)
        .to_radians() as f32;
    let key_intensity = style_number(
        &props,
        "key-light-intensity",
        settings.key_intensity as f32,
        [0., 20.],
    );
    let fill_intensity = style_number(
        &props,
        "fill-light-intensity",
        settings.fill_intensity as f32,
        [0., 20.],
    );
    let exposure = style_number(&props, "exposure", settings.exposure as f32, [-10., 10.]);
    let shadows = props
        .get("shadows")
        .map(|value| value != "false" && value != "none")
        .unwrap_or(settings.shadows);
    if let Ok((entity, mut camera_exposure, ssao)) = camera.single_mut() {
        camera_exposure.ev100 = 10.0 - exposure;
        let radius = style_number(&props, "ambient-occlusion-radius", 0.15, [0.001, 10.]);
        if shadows {
            if let Some(mut ssao) = ssao {
                ssao.radius = radius;
            } else {
                commands.entity(entity).insert(ScreenSpaceAmbientOcclusion {
                    radius,
                    quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
                    constant_object_thickness: 0.01,
                });
            }
        } else if ssao.is_some() {
            commands
                .entity(entity)
                .remove::<ScreenSpaceAmbientOcclusion>();
        }
        if let Some((diffuse, specular, compensation)) = runtime.environment_maps() {
            commands.entity(entity).insert(EnvironmentMapLight {
                diffuse_map: diffuse.clone(),
                specular_map: specular.clone(),
                intensity: STUDIO_INTENSITY * environment * compensation,
                // Bevy rotates lookup rays, rather than the cubemap itself.
                // Use the inverse so cards and direct lights turn together.
                rotation: Quat::from_rotation_y(-rotation),
                ..default()
            });
        }
    }
    let target = Vec3::from_array(w.view.target);
    let distance = w.view.distance.max(0.001);
    let rotation = Quat::from_rotation_y(rotation);
    for (entity, mut light, mut transform, role) in &mut lights {
        commands.entity(entity).insert(shadow_config(distance));
        light.illuminance = if role.0 {
            KEY_ILLUMINANCE * key_intensity
        } else {
            FILL_ILLUMINANCE * fill_intensity
        };
        light.shadow_maps_enabled = shadows && role.0;
        light.shadow_normal_bias = 0.015;
        let direction = rotation
            * if role.0 {
                KEY_DIRECTION
            } else {
                FILL_DIRECTION
            };
        *transform = Transform::from_translation(target + direction.normalize() * distance * 4.)
            .looking_at(target, Vec3::Y);
    }
}

pub fn shadow_config(distance: f32) -> bevy::light::CascadeShadowConfig {
    let reach = distance.max(0.02) * 8.;
    CascadeShadowConfigBuilder {
        num_cascades: 3,
        minimum_distance: 0.0001,
        maximum_distance: reach,
        first_cascade_far_bound: reach * 0.1,
        ..default()
    }
    .build()
}

pub struct SurfaceStyle {
    pub tint: Option<Color>,
    pub translucent: bool,
    pub two_sided: bool,
}

pub fn material(
    doc: &Document,
    finish: &Finish,
    instance: &PlacedPart,
    root: &Path,
    runtime: &mut Runtime,
    style: SurfaceStyle,
) -> FinishMaterial {
    let SurfaceStyle {
        tint: color,
        translucent,
        two_sided,
    } = style;
    let [r, g, b] = finish.base_color.map(|value| value as f32);
    let opaque_color = Color::srgb(r, g, b);
    let base_color = color.unwrap_or(opaque_color);
    let mut extension = extension(finish, instance);
    let white = runtime.white.clone();
    let normal = runtime.flat_normal.clone();
    let mut flags = 0_u32;
    let mut map = |id: Option<&str>, srgb: bool, flag: u32, fallback: &Handle<Image>| {
        if let Some(image) = runtime.texture(doc, root, id, srgb, flag == 32) {
            flags |= flag;
            image
        } else {
            fallback.clone()
        }
    };
    extension.color = map(finish.textures.base_color.as_deref(), true, 1, &white);
    extension.normal = map(finish.textures.normal.as_deref(), false, 2, &normal);
    extension.roughness = map(finish.textures.roughness.as_deref(), false, 4, &white);
    extension.metallic = map(finish.textures.metallic.as_deref(), false, 8, &white);
    extension.occlusion = map(finish.textures.occlusion.as_deref(), false, 16, &white);
    extension.height = map(finish.textures.height.as_deref(), false, 32, &white);
    // An explicitly assigned image owns the bump slot even while loading or
    // after a decode error. Do not silently layer or substitute built-in grain.
    if finish.textures.height.is_none()
        && let Some(TextureState::Loaded(height)) = runtime.builtin_height.get(&finish.builtin_bump)
    {
        extension.height = height.clone();
        flags |= 32;
    }
    extension.uniform.relief.y = flags as f32;
    // Draft/comparison tint is a semantic overlay; it does not rewrite visual finish source.
    let base = StandardMaterial {
        base_color,
        metallic: if translucent {
            0.08
        } else {
            finish.metallic as f32
        },
        perceptual_roughness: finish.roughness as f32,
        anisotropy_strength: if translucent {
            0.
        } else {
            finish.anisotropy as f32
        },
        reflectance: 0.5,
        clearcoat: if translucent {
            0.
        } else {
            finish.clearcoat as f32
        },
        clearcoat_perceptual_roughness: finish.clearcoat_roughness as f32,
        alpha_mode: if translucent {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        double_sided: two_sided,
        cull_mode: if two_sided { None } else { Some(Face::Back) },
        ..default()
    };
    FinishMaterial { base, extension }
}

fn extension(finish: &Finish, instance: &PlacedPart) -> FinishExtension {
    // One render unit is 100 CAD mm; positions map by [x,z,-y] * 0.01.
    let matrix = &instance.matrix;
    let part_to_world = DMat4::from_cols_array(&std::array::from_fn::<_, 16, _>(|index| {
        let col = index / 4;
        match index % 4 {
            0 => matrix[0][col] * 0.01,
            1 => matrix[2][col] * 0.01,
            2 => -matrix[1][col] * 0.01,
            _ => {
                if col == 3 {
                    1.
                } else {
                    0.
                }
            }
        }
    }));
    let (sine, cosine) = finish.mapping.rotation_deg.to_radians().sin_cos();
    FinishExtension {
        uniform: FinishUniform {
            world_to_part: part_to_world.inverse().as_mat4(),
            part_to_world: part_to_world.as_mat4(),
            mapping: Vec4::new(
                finish.mapping.scale_mm[0] as f32,
                finish.mapping.scale_mm[1] as f32,
                cosine as f32,
                sine as f32,
            ),
            settings: Vec4::new(
                finish.mapping.offset_mm[0] as f32,
                finish.mapping.offset_mm[1] as f32,
                finish.normal_strength as f32,
                if finish.mapping.mode == MappingMode::Planar {
                    1.
                } else {
                    0.
                },
            ),
            relief: Vec4::new(finish.height_mm as f32, 0., 0., 0.),
            surface: Vec4::new(0., 0., finish.anisotropy as f32, 0.),
        },
        color: Handle::default(),
        normal: Handle::default(),
        roughness: Handle::default(),
        metallic: Handle::default(),
        occlusion: Handle::default(),
        height: Handle::default(),
    }
}

fn pixel(value: [u8; 4]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &value,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

// Code-native studio resources: no network, external asset bundle or photographic content.
struct StudioCard {
    axis: Vec3,
    side: Vec3,
    up: Vec3,
    inverse_widths: Vec2,
    radiance: Vec3,
}
fn studio_cards() -> &'static [StudioCard; 5] {
    static CARDS: OnceLock<[StudioCard; 5]> = OnceLock::new();
    CARDS.get_or_init(|| {
        [
            (
                KEY_DIRECTION,
                Vec3::X,
                [0.75, 0.40],
                3.8,
                Vec3::new(1., 0.97, 0.92),
            ),
            (
                FILL_DIRECTION,
                Vec3::Y,
                [0.62, 0.95],
                2.2,
                Vec3::new(0.88, 0.94, 1.),
            ),
            (
                Vec3::new(-4., 1., -3.),
                Vec3::Y,
                [0.38, 1.20],
                2.5,
                Vec3::new(0.98, 0.99, 1.),
            ),
            (Vec3::new(2., 2., 5.), Vec3::Y, [0.30, 0.90], 2.6, Vec3::ONE),
            (Vec3::new(2., 6., 1.), Vec3::Z, [0.95, 0.36], 2.4, Vec3::ONE),
        ]
        .map(|(axis, tangent, widths, strength, color)| {
            let axis = axis.normalize();
            let side = tangent.cross(axis).normalize();
            StudioCard {
                axis,
                side,
                up: axis.cross(side),
                inverse_widths: Vec2::new(1. / widths[0], 1. / widths[1]),
                radiance: color * strength,
            }
        })
    })
}
fn studio_radiance(direction: Vec3) -> Vec3 {
    // Neutral floor and continuous bright walls provide reflected fill from
    // every side. A metal has almost no diffuse response, so directional fill
    // alone cannot illuminate a face whose reflection misses a small softbox.
    let wall = (direction.y + 1.).clamp(0., 1.);
    let ceiling = direction.y.max(0.).powi(2);
    let mut radiance = Vec3::new(0.36, 0.37, 0.39).lerp(Vec3::new(0.80, 0.84, 0.90), wall)
        + Vec3::new(0.24, 0.25, 0.25) * ceiling;
    for card in studio_cards() {
        let facing = direction.dot(card.axis);
        if facing > 0. {
            let uv = Vec2::new(direction.dot(card.side), direction.dot(card.up))
                * card.inverse_widths
                / facing;
            // Beyond this bound the eighth-power falloff contributes < 3e-9.
            if uv.abs().max_element() > 1.45 {
                continue;
            }
            // Smooth rectangle softboxes preserve recognizable highlights without aliasing hard rims.
            let squared = uv * uv;
            let fourth = squared * squared;
            let eighth = fourth * fourth;
            let shape = (-eighth.element_sum()).exp();
            radiance += card.radiance * shape;
        }
    }
    radiance
}
fn studio_maps() -> (Image, Image) {
    (
        crate::environment_filter::cube_image(32, 1, true, &studio_radiance),
        crate::environment_filter::cube_image(128, 8, false, &studio_radiance),
    )
}
