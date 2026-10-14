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
    surface: vec4<f32>, // reserved, reserved, anisotropy strength, reserved
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
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    // Differentiate the filtered height field at the current pixel footprint. A base-level
    // texel step becomes numerically tiny at distant or grazing views and exaggerates aliasing.
    // Per-axis steps retain directional grain; explicit gradients keep the four samples on
    // the same mip footprint and let the sampler filter anisotropically.
    let footprint = vec2<f32>(length(vec2<f32>(dx.x, dy.x)), length(vec2<f32>(dx.y, dy.y)));
    let step = max(1.0 / dimensions, footprint * 0.5);
    let left = textureSampleGrad(height_map, color_sampler, uv - vec2<f32>(step.x, 0.0), dx, dy).r;
    let right = textureSampleGrad(height_map, color_sampler, uv + vec2<f32>(step.x, 0.0), dx, dy).r;
    let bottom = textureSampleGrad(height_map, color_sampler, uv - vec2<f32>(0.0, step.y), dx, dy).r;
    let top = textureSampleGrad(height_map, color_sampler, uv + vec2<f32>(0.0, step.y), dx, dy).r;
    // Preserve physical relief amplitude independently of image resolution, mip level,
    // texture tile size and document units. Height is lighting detail, never CAD displacement.
    let slope = -vec2<f32>(right - left, top - bottom) * finish.relief.x / (2.0 * step * finish.mapping.xy);
    let c = finish.mapping.z;
    let s = finish.mapping.w;
    return (tangent * c + bitangent * s) * slope.x + (-tangent * s + bitangent * c) * slope.y;
}

fn mapped_tangent(tangent: vec3<f32>, bitangent: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let direction = tangent * finish.mapping.z + bitangent * finish.mapping.w;
    return direction - normal * dot(direction, normal);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let point = (finish.world_to_part * in.world_position).xyz;
    // Normals are covectors. Using the position/vector inverse skews relief and
    // reflections on scaled/sheared occurrences. UVs still use world_to_part.
    let world_normal_to_part = transpose(finish.part_to_world);
    let part_normal_to_world = transpose(finish.world_to_part);
    let local_n = normalize((world_normal_to_part * vec4<f32>(pbr.N, 0.0)).xyz);
    let geometric_n = normalize((world_normal_to_part * vec4<f32>(in.world_normal, 0.0)).xyz);
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
    pbr.N = normalize((part_normal_to_world * vec4<f32>(mapped_n, 0.0)).xyz);
    // The coating follows the carrier and imported normal map, while the grayscale
    // machining relief belongs to the underlying finish. Keep its broad reflection smooth.
    let tangent_normal = normal_detail - geometric_n * dot(normal_detail, geometric_n);
    let coat_n = normalize(local_n + tangent_normal * facing);
    pbr.clearcoat_N = normalize((part_normal_to_world * vec4<f32>(coat_n, 0.0)).xyz);
    @if(STANDARD_MATERIAL_ANISOTROPY) {
        // Supply physical part-local directions directly: tessellation UVs and vertex
        // tangents do not define the appearance mapping and may be absent entirely.
        var tx = mapped_tangent(vec3<f32>(0.0, signs.x, 0.0), vec3<f32>(0.0, 0.0, 1.0), mapped_n);
        var ty = mapped_tangent(vec3<f32>(0.0, 0.0, signs.y), vec3<f32>(1.0, 0.0, 0.0), mapped_n);
        var tz = mapped_tangent(vec3<f32>(signs.z, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), mapped_n);
        var reference = tz;
        if weights.x > max(weights.y, weights.z) {
            reference = tx;
        } else if weights.y > weights.z {
            reference = ty;
        }
        // A brush direction is an unoriented axis. Align the axes before blending so
        // opposite projections cannot cancel and create a singular highlight direction.
        tx *= select(-1.0, 1.0, dot(tx, reference) >= 0.0);
        ty *= select(-1.0, 1.0, dot(ty, reference) >= 0.0);
        tz *= select(-1.0, 1.0, dot(tz, reference) >= 0.0);
        var tangent = tx * weights.x + ty * weights.y + tz * weights.z;
        // The resolved height gradient runs across the grain. Its perpendicular
        // follows the imported/built-in image, including texture rotation and
        // irregular machining marks, rather than imposing an unrelated stripe.
        // Keep the mapped direction where a locally flat texel has no evidence.
        var grain = cross(geometric_n, height_normal);
        if (flags & 32u) != 0u && dot(grain, grain) > 0.000001 {
            grain = normalize(grain);
            grain *= select(-1.0, 1.0, dot(grain, tangent) >= 0.0);
            tangent = mix(tangent / max(length(tangent), 0.00001), grain, 0.65);
        }
        if dot(tangent, tangent) < 0.000001 {
            let axis = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(mapped_n.x) > 0.8);
            tangent = cross(mapped_n, axis);
        }
        let world_tangent = (finish.part_to_world * vec4<f32>(tangent, 0.0)).xyz;
        pbr.anisotropy_T = normalize(world_tangent - pbr.N * dot(world_tangent, pbr.N));
        pbr.anisotropy_B = normalize(cross(pbr.N, pbr.anisotropy_T));
        pbr.anisotropy_strength = clamp(finish.surface.z, 0.0, 1.0);
    }
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * color.rgb, pbr.material.base_color.a);
    let authored_roughness = clamp(pbr.material.perceptual_roughness * roughness, 0.045, 1.0);
    // Mip-filtered heights retain the resolved grain direction, while rapidly
    // varying bump slopes within one screen pixel need a broader reflection.
    // Bound this display-only variance estimate; it never changes finish source
    // roughness, bump depth or engineering surface-finish requirements.
    let bump_dx = dpdx(tangent_bump);
    let bump_dy = dpdy(tangent_bump);
    let variance = min(0.1, 0.25 * (dot(bump_dx, bump_dx) + dot(bump_dy, bump_dy)));
    let alpha = authored_roughness * authored_roughness;
    pbr.material.perceptual_roughness = clamp(sqrt(sqrt(alpha * alpha + variance)), 0.045, 1.0);
    pbr.material.metallic *= metallic;
    pbr.diffuse_occlusion *= vec3<f32>(occlusion);
    pbr.specular_occlusion *= occlusion;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
