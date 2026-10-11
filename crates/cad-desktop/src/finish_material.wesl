// Renderer adapter only. CAD source retains f64 physical texture scale and occurrence transforms.
import constants;
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct FinishUniform {
    world_to_part: mat4x4<f32>,
    part_to_world: mat4x4<f32>,
    mapping: vec4<f32>, // scale U/V mm, rotation cosine/sine
    settings: vec4<f32>, // offset U/V mm, normal strength, planar flag
    relief: vec4<f32>, // height range in physical mm
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> finish: FinishUniform;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var color_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var roughness_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var metallic_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var occlusion_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var height_map: texture_2d<f32>;

fn finish_uv(position: vec2<f32>) -> vec2<f32> {
    let centered = position - finish.settings.xy;
    let c = finish.mapping.z;
    let s = finish.mapping.w;
    return vec2<f32>(c * centered.x + s * centered.y, -s * centered.x + c * centered.y) / finish.mapping.xy;
}

// OpenGL tangent normal maps (+Y). Rotate tangent axes with physical texture direction.
fn detail(normal: vec3<f32>, tangent: vec3<f32>, bitangent: vec3<f32>) -> vec3<f32> {
    let c = finish.mapping.z;
    let s = finish.mapping.w;
    let t = tangent * c + bitangent * s;
    let b = -tangent * s + bitangent * c;
    let slope = normal.xy / max(normal.z, 0.05);
    return (t * slope.x + b * slope.y) * finish.settings.z;
}

fn height_detail(uv: vec2<f32>, tangent: vec3<f32>, bitangent: vec3<f32>) -> vec3<f32> {
    let dimensions = vec2<f32>(textureDimensions(height_map));
    let texel = 1.0 / dimensions;
    let left = textureSample(height_map, color_sampler, uv - vec2<f32>(texel.x, 0.0)).r;
    let right = textureSample(height_map, color_sampler, uv + vec2<f32>(texel.x, 0.0)).r;
    let bottom = textureSample(height_map, color_sampler, uv - vec2<f32>(0.0, texel.y)).r;
    let top = textureSample(height_map, color_sampler, uv + vec2<f32>(0.0, texel.y)).r;
    // Preserve the requested relief amplitude as texture tile size changes.
    let slope = -vec2<f32>(right - left, top - bottom) * finish.relief.x * dimensions / (2.0 * finish.mapping.xy);
    let c = finish.mapping.z;
    let s = finish.mapping.w;
    return (tangent * c + bitangent * s) * slope.x + (-tangent * s + bitangent * c) * slope.y;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let point = (finish.world_to_part * in.world_position).xyz;
    let local_n = normalize((finish.world_to_part * vec4<f32>(pbr.N, 0.0)).xyz);
    let geometric_n = normalize((finish.world_to_part * vec4<f32>(in.world_normal, 0.0)).xyz);
    var signs = select(vec3<f32>(-1.0), vec3<f32>(1.0), geometric_n >= vec3<f32>(0.0));
    var weights = pow(abs(geometric_n), vec3<f32>(4.0));
    weights /= max(dot(weights, vec3<f32>(1.0)), 0.00001);
    if finish.settings.w > 0.5 {
        weights = vec3<f32>(0.0, 0.0, 1.0);
        signs = vec3<f32>(1.0);
    }
    let uv_x = finish_uv(vec2<f32>(point.y * signs.x, point.z));
    let uv_y = finish_uv(vec2<f32>(point.z * signs.y, point.x));
    let uv_z = finish_uv(vec2<f32>(point.x * signs.z, point.y));
    let flags = u32(finish.relief.y);
    var color = vec4<f32>(1.0);
    if (flags & 1u) != 0u {
        color = textureSample(color_map, color_sampler, uv_x) * weights.x
            + textureSample(color_map, color_sampler, uv_y) * weights.y
            + textureSample(color_map, color_sampler, uv_z) * weights.z;
    }
    var roughness = 1.0;
    if (flags & 4u) != 0u {
        roughness = textureSample(roughness_map, color_sampler, uv_x).r * weights.x
            + textureSample(roughness_map, color_sampler, uv_y).r * weights.y
            + textureSample(roughness_map, color_sampler, uv_z).r * weights.z;
    }
    var metallic = 1.0;
    if (flags & 8u) != 0u {
        metallic = textureSample(metallic_map, color_sampler, uv_x).r * weights.x
            + textureSample(metallic_map, color_sampler, uv_y).r * weights.y
            + textureSample(metallic_map, color_sampler, uv_z).r * weights.z;
    }
    var occlusion = 1.0;
    if (flags & 16u) != 0u {
        occlusion = textureSample(occlusion_map, color_sampler, uv_x).r * weights.x
            + textureSample(occlusion_map, color_sampler, uv_y).r * weights.y
            + textureSample(occlusion_map, color_sampler, uv_z).r * weights.z;
    }
    var normal_detail = vec3<f32>(0.0);
    if (flags & 2u) != 0u {
        let normal_x = textureSample(normal_map, color_sampler, uv_x).xyz * 2.0 - 1.0;
        let normal_y = textureSample(normal_map, color_sampler, uv_y).xyz * 2.0 - 1.0;
        let normal_z = textureSample(normal_map, color_sampler, uv_z).xyz * 2.0 - 1.0;
        let dx = detail(normal_x, vec3<f32>(0.0, signs.x, 0.0), vec3<f32>(0.0, 0.0, 1.0));
        let dy = detail(normal_y, vec3<f32>(0.0, 0.0, signs.y), vec3<f32>(1.0, 0.0, 0.0));
        let dz = detail(normal_z, vec3<f32>(signs.z, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
        normal_detail = dx * weights.x + dy * weights.y + dz * weights.z;
    }
    var height_normal = vec3<f32>(0.0);
    if (flags & 32u) != 0u {
        let hx = height_detail(uv_x, vec3<f32>(0.0, signs.x, 0.0), vec3<f32>(0.0, 0.0, 1.0));
        let hy = height_detail(uv_y, vec3<f32>(0.0, 0.0, signs.y), vec3<f32>(1.0, 0.0, 0.0));
        let hz = height_detail(uv_z, vec3<f32>(signs.z, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
        height_normal = hx * weights.x + hy * weights.y + hz * weights.z;
    }
    let bump = normal_detail + height_normal;
    let tangent_bump = bump - geometric_n * dot(bump, geometric_n);
    let facing = select(-1.0, 1.0, dot(local_n, geometric_n) >= 0.0);
    let mapped_n = normalize(local_n + tangent_bump * facing);
    pbr.N = normalize((finish.part_to_world * vec4<f32>(mapped_n, 0.0)).xyz);
    pbr.clearcoat_N = pbr.N;
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * color.rgb, pbr.material.base_color.a);
    pbr.material.perceptual_roughness = clamp(pbr.material.perceptual_roughness * roughness, 0.045, 1.0);
    pbr.material.metallic *= metallic;
    pbr.diffuse_occlusion *= vec3<f32>(occlusion);
    pbr.specular_occlusion *= occlusion;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
