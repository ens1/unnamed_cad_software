//! Disposable CPU image-based-lighting caches for the replaceable Bevy adapter.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
};
use half::f16;
use std::sync::OnceLock;
pub(crate) struct Filtered {
    pub diffuse: Image,
    pub specular: Image,
    pub intensity_compensation: f32,
    pub normalization: f32,
    pub source_dimensions: [u32; 2],
    pub maximum_radiance: f32,
}
pub(crate) fn panorama(source: &crate::environment_assets::Panorama) -> Filtered {
    let normalization = if source.maximum > 16_000. {
        16_000. / source.maximum
    } else {
        1.
    };
    let radiance = |direction| source.sample(direction) * normalization;
    Filtered {
        diffuse: cube_image(32, 1, true, &radiance),
        specular: cube_image(128, 8, false, &radiance),
        intensity_compensation: 1. / normalization,
        normalization,
        source_dimensions: [source.width, source.height],
        maximum_radiance: source.maximum,
    }
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
    // Match Bevy's left-handed cube lookup to the authored world direction.
    Vec3::new(direction.x, direction.y, -direction.z).normalize()
}
fn filter_samples() -> &'static [Vec3; 256] {
    static SAMPLES: OnceLock<[Vec3; 256]> = OnceLock::new();
    SAMPLES.get_or_init(|| {
        std::array::from_fn(|sample| {
            let x = (sample as f32 + 0.5) / 256.;
            let y = (sample as u32).reverse_bits() as f32 * 2.328_306_4e-10;
            let phi = std::f32::consts::TAU * x;
            Vec3::new(phi.cos(), phi.sin(), y)
        })
    })
}
fn filtered(normal: Vec3, roughness: f32, diffuse: bool, radiance: &impl Fn(Vec3) -> Vec3) -> Vec3 {
    if !diffuse && roughness == 0. {
        return radiance(normal);
    }
    let tangent = if normal.z.abs() < 0.99 {
        Vec3::Z.cross(normal).normalize()
    } else {
        Vec3::X.cross(normal).normalize()
    };
    let bitangent = normal.cross(tangent);
    let mut sum = Vec3::ZERO;
    let mut weight = 0.;
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
        sum += radiance(direction) * sample_weight;
        weight += sample_weight;
    }
    sum / weight.max(0.0001)
}
pub(crate) fn cube_image(
    size: u32,
    levels: u32,
    diffuse: bool,
    radiance: &impl Fn(Vec3) -> Vec3,
) -> Image {
    let texels = (0..levels)
        .map(|mip| (size >> mip).max(1).pow(2) as usize * 6)
        .sum::<usize>();
    let mut bytes = Vec::with_capacity(texels * 8);
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
                    let color = filtered(
                        cube_direction(face, x, y, width),
                        roughness,
                        diffuse,
                        radiance,
                    );
                    for channel in [color.x, color.y, color.z, 1.] {
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
