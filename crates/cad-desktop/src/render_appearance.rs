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
        AsBindGroup, Extent3d, Face, ShaderType, TextureDataOrder, TextureDimension, TextureFormat,
        TextureViewDescriptor, TextureViewDimension,
    },
    shader::{Shader, ShaderRef},
};
use cad_core::{
    Document, PlacedPart,
    appearance::{BuiltinBump, Finish, MappingMode},
};
use half::f16;
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
enum Reply {
    Texture(TextureKey, Result<Image, String>),
    BuiltinHeight(Result<Image, String>),
    Studio(Image, Image),
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
    builtin_height: TextureState,
    pub epoch: u64,
    studio: Option<(Handle<Image>, Handle<Image>)>,
    usage_key: String,
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
            builtin_height: TextureState::Pending,
            epoch: 0,
            studio: None,
            usage_key: String::new(),
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
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn diagnostics(&self, doc: &Document, root: &Path, validating: bool) -> Vec<String> {
        let mut messages = vec![];
        if doc.finishes.values().any(|finish| {
            finish.textures.height.is_none() && finish.builtin_bump == BuiltinBump::Brushed
        }) {
            match &self.builtin_height {
                TextureState::Pending => {
                    messages.push("Loading texture built-in brushed grain".into())
                }
                TextureState::Failed(error) => {
                    messages.push(format!("Built-in brushed bump: {error}"))
                }
                TextureState::Loaded(_) => {}
            }
        }
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
        messages
    }
}

#[derive(Component)]
pub struct StudioLight(pub bool);

pub fn setup(commands: &mut Commands, images: &mut Assets<Image>, runtime: &mut Runtime) {
    runtime.white = images.add(pixel([255, 255, 255, 255]));
    runtime.flat_normal = images.add(pixel([128, 128, 255, 255]));
    let height_sender = runtime.sender.clone();
    std::thread::spawn(move || {
        let result = crate::texture_assets::builtin_brushed_height();
        let _ = height_sender.send(Reply::BuiltinHeight(result));
    });
    // A coarse studio is available immediately. Filtering the final resources
    // stays off the main thread, but metal never starts in an unlit void.
    runtime.studio = Some((
        images.add(cube_image(4, 1, true)),
        images.add(cube_image(8, 4, false)),
    ));
    let sender = runtime.sender.clone();
    std::thread::spawn(move || {
        let (diffuse, specular) = studio_maps();
        let _ = sender.send(Reply::Studio(diffuse, specular));
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
        "{}:{}:{}:{:?}:{:?}:{}",
        w.session.generation,
        w.session.committed.revision,
        w.saved_source,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit)),
        w.root(),
        w.fields.contains_key("appearance-tool")
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
        "{}:{}:{}:{:?}:{}:{:?}",
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
    let validating = w.fields.contains_key("appearance-tool");
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
            Reply::BuiltinHeight(result) => {
                runtime.builtin_height = match result {
                    Ok(image) => {
                        runtime.image_bytes += image.data.as_ref().map(Vec::len).unwrap_or(0);
                        TextureState::Loaded(images.add(image))
                    }
                    Err(error) => TextureState::Failed(error),
                };
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
            let _ = sender.send(Reply::Texture(request.key, result));
        });
    }
    w.appearance_diagnostics = runtime.diagnostics(w.session.visible(), &w.root(), validating);
    w.appearance_validation_key = validation_key(&w);
    if validating && w.confirm_requested && resources_are_current(&w) {
        if w.appearance_diagnostics.is_empty() {
            // Resource completion may follow geometry completion, without another worker reply.
            w.confirm_tool();
        } else if !resources_pending(&w) {
            w.confirm_requested = false;
            w.status = "Resolve the current texture resource errors before applying".into();
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
        if let Some((diffuse, specular)) = &runtime.studio {
            commands.entity(entity).insert(EnvironmentMapLight {
                diffuse_map: diffuse.clone(),
                specular_map: specular.clone(),
                intensity: STUDIO_INTENSITY * environment,
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
        && finish.builtin_bump == BuiltinBump::Brushed
        && let TextureState::Loaded(height) = &runtime.builtin_height
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
fn cube_direction(face: u32, x: u32, y: u32, size: u32) -> Vec3 {
    let u = 2. * (x as f32 + 0.5) / size as f32 - 1.;
    let v = 2. * (y as f32 + 0.5) / size as f32 - 1.;
    let direction = match face {
        0 => Vec3::new(1., -v, -u),
        1 => Vec3::new(-1., -v, u),
        2 => Vec3::new(u, 1., v),
        3 => Vec3::new(u, -1., -v),
        4 => Vec3::new(u, -v, 1.),
        _ => Vec3::new(-u, -v, -1.),
    };
    // Texture cubemaps are left-handed; Bevy's PBR lookup reverses world Z.
    // Evaluate the studio in world axes so its softboxes match direct lights.
    Vec3::new(direction.x, direction.y, -direction.z).normalize()
}
fn sample_pair(sample: u32, count: u32) -> Vec2 {
    Vec2::new(
        (sample as f32 + 0.5) / count as f32,
        sample.reverse_bits() as f32 * 2.328_306_4e-10,
    )
}
fn filter_samples() -> &'static [Vec3; 256] {
    static SAMPLES: OnceLock<[Vec3; 256]> = OnceLock::new();
    SAMPLES.get_or_init(|| {
        std::array::from_fn(|sample| {
            let pair = sample_pair(sample as u32, 256);
            let phi = std::f32::consts::TAU * pair.x;
            Vec3::new(phi.cos(), phi.sin(), pair.y)
        })
    })
}
fn tangent_frame(normal: Vec3) -> (Vec3, Vec3) {
    let tangent = if normal.z.abs() < 0.99 {
        Vec3::Z.cross(normal).normalize()
    } else {
        Vec3::X.cross(normal).normalize()
    };
    (tangent, normal.cross(tangent))
}
fn filtered_radiance(normal: Vec3, roughness: f32, diffuse: bool) -> Vec3 {
    if !diffuse && roughness == 0. {
        return studio_radiance(normal);
    }
    let (tangent, bitangent) = tangent_frame(normal);
    let mut sum = Vec3::ZERO;
    let mut weight = 0.;
    // More samples reduce seams and speckled reflections on rough finishes.
    for sample in filter_samples() {
        let cosine = if diffuse {
            (1. - sample.z).sqrt()
        } else {
            let alpha_squared = roughness.powi(4);
            ((1. - sample.z) / (1. + (alpha_squared - 1.) * sample.z)).sqrt()
        };
        let sine = (1. - cosine * cosine).max(0.).sqrt();
        let half = tangent * (sample.x * sine) + bitangent * (sample.y * sine) + normal * cosine;
        let direction = if diffuse {
            half
        } else {
            2. * normal.dot(half) * half - normal
        };
        let sample_weight = if diffuse {
            1.
        } else {
            normal.dot(direction).max(0.)
        };
        sum += studio_radiance(direction) * sample_weight;
        weight += sample_weight;
    }
    sum / weight.max(0.0001)
}
fn cube_image(size: u32, levels: u32, diffuse: bool) -> Image {
    let texels = (0..levels)
        .map(|mip| (size >> mip).max(1).pow(2) as usize * 6)
        .sum::<usize>();
    let mut bytes = Vec::with_capacity(texels * 8);
    // Explicit mip-major ordering matches wgpu upload order across the six cubemap faces.
    for mip in 0..levels {
        let width = (size >> mip).max(1);
        let roughness = if levels > 1 {
            mip as f32 / (levels - 1) as f32
        } else {
            1.
        };
        for face in 0..6 {
            for y in 0..width {
                for x in 0..width {
                    let radiance =
                        filtered_radiance(cube_direction(face, x, y, width), roughness, diffuse);
                    for channel in [radiance.x, radiance.y, radiance.z, 1.] {
                        bytes.extend_from_slice(&f16::from_f32(channel).to_le_bytes());
                    }
                }
            }
        }
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    image.data = Some(bytes);
    image.texture_descriptor.mip_level_count = levels;
    image.data_order = TextureDataOrder::MipMajor;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}
fn studio_maps() -> (Image, Image) {
    (cube_image(32, 1, true), cube_image(128, 8, false))
}
