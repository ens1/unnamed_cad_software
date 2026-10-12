//! Bounded portable texture decoding for the replaceable desktop renderer.
//! Color maps are sRGB; scalar/normal maps stay linear. No CAD geometry is edited.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::Image,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cad_core::{CadError, Resource};
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

const MAX_ENCODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RGBA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODER_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;

fn format(kind: &str) -> Result<ImageFormat, String> {
    match kind.to_ascii_lowercase().as_str() {
        "png" => Ok(ImageFormat::Png),
        "jpg" | "jpeg" => Ok(ImageFormat::Jpeg),
        "tif" | "tiff" => Ok(ImageFormat::Tiff),
        "webp" => Ok(ImageFormat::WebP),
        _ => Err("Choose a PNG, JPEG, TIFF or WebP texture file".into()),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("Cannot read texture: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect texture: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ENCODED_BYTES {
        return Err("Texture must be a nonempty regular file no larger than 64 MiB".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_ENCODED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read texture: {error}"))?;
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err("Texture grew beyond the 64 MiB encoded-file limit".into());
    }
    Ok(bytes)
}

fn reader<'a>(bytes: &'a [u8], kind: &str) -> Result<ImageReader<Cursor<&'a [u8]>>, String> {
    let expected = format(kind)?;
    let actual =
        image::guess_format(bytes).map_err(|error| format!("Cannot identify texture: {error}"))?;
    if actual != expected {
        return Err("Texture contents do not match the file extension; use its actual PNG/JPEG/TIFF/WebP extension".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), actual);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODER_BYTES);
    reader.limits(limits);
    Ok(reader)
}

fn dimensions(bytes: &[u8], kind: &str) -> Result<(u32, u32), String> {
    let (width, height) = reader(bytes, kind)?
        .into_dimensions()
        .map_err(|error| format!("Cannot read texture dimensions: {error}"))?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) * 4 > MAX_RGBA_BYTES
    {
        return Err("Texture exceeds 8192 pixels on one axis or 64 MiB of RGBA pixels; use a smaller image (4096×4096 is supported)".into());
    }
    Ok((width, height))
}

fn decode(bytes: &[u8], kind: &str) -> Result<DynamicImage, String> {
    let (width, height) = dimensions(bytes, kind)?;
    let decoded = reader(bytes, kind)?
        .decode()
        .map_err(|error| format!("Cannot decode texture: {error}"))?;
    if decoded.width() != width || decoded.height() != height {
        return Err("Texture dimensions changed while decoding".into());
    }
    Ok(decoded)
}

/// Read and validate the exact bounded bytes to pin in a draft resource. The
/// source is not reopened after validation. Full decoding runs in the asset
/// worker, and the current readiness stamp gates Apply on its result.
pub(crate) fn validate_texture_file(path: &Path) -> cad_core::Result<Vec<u8>> {
    let kind = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    read(path)
        .and_then(|bytes| dimensions(&bytes, kind).map(|_| bytes))
        .map_err(|message| CadError::Invalid(format!("Texture: {message}")))
}

/// Personal new-document defaults validate the exact pinned package bytes
/// before saving them. Ordinary viewport imports still decode in their worker.
pub(crate) fn validate_default_texture(bytes: &[u8], kind: &str) -> cad_core::Result<()> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err(CadError::Invalid(
            "Default finish texture exceeds the encoded image bound".into(),
        ));
    }
    decode(bytes, kind)
        .map(|_| ())
        .map_err(|message| CadError::Invalid(format!("Default finish texture: {message}")))
}

fn linear(value: u8) -> f64 {
    let value = f64::from(value) / 255.;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(value: f64) -> u8 {
    let value = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    };
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn mips(base: RgbaImage, is_srgb: bool) -> (Vec<u8>, u32) {
    let (mut width, mut height) = base.dimensions();
    let mut data = base.into_raw();
    let mut offset = 0;
    let mut levels = 1;
    while width > 1 || height > 1 {
        let next_width = (width / 2).max(1);
        let next_height = (height / 2).max(1);
        let mut next = vec![0; next_width as usize * next_height as usize * 4];
        for y in 0..next_height {
            for x in 0..next_width {
                let x0 = x * width / next_width;
                let x1 = (x + 1) * width / next_width;
                let y0 = y * height / next_height;
                let y1 = (y + 1) * height / next_height;
                let count = f64::from((x1 - x0) * (y1 - y0));
                let mut sum = [0_f64; 4];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let index = offset + (sy as usize * width as usize + sx as usize) * 4;
                        for channel in 0..4 {
                            let value = data[index + channel];
                            sum[channel] += if is_srgb && channel < 3 {
                                linear(value)
                            } else {
                                f64::from(value) / 255.
                            };
                        }
                    }
                }
                let index = (y as usize * next_width as usize + x as usize) * 4;
                for channel in 0..4 {
                    let mean = sum[channel] / count;
                    next[index + channel] = if is_srgb && channel < 3 {
                        srgb(mean)
                    } else {
                        (mean.clamp(0., 1.) * 255.).round() as u8
                    };
                }
            }
        }
        offset = data.len();
        data.extend(next);
        width = next_width;
        height = next_height;
        levels += 1;
    }
    (data, levels)
}

/// Load a checksum-pinned project texture; intended for the renderer's asset worker.
pub(crate) fn load(root: &Path, resource: &Resource, is_srgb: bool) -> Result<Image, String> {
    let path = cad_core::persistence::safe_join(root, &resource.path)
        .map_err(|error| error.to_string())?;
    let bytes = read(&path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != resource.sha256 {
        return Err(format!(
            "Texture {} changed; reimport the intended file",
            resource.path
        ));
    }
    let rgba = decode(&bytes, &resource.kind)?.into_rgba8();
    let (width, height) = rgba.dimensions();
    let (data, levels) = mips(rgba, is_srgb);
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        if is_srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::default(),
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Ok(image)
}

/// Preserve high-bit-depth grayscale relief until conversion to filterable GPU half floats.
/// Red is the authoritative height channel, independent of any image color-space metadata.
pub(crate) fn load_height(root: &Path, resource: &Resource) -> Result<Image, String> {
    let path = cad_core::persistence::safe_join(root, &resource.path)
        .map_err(|error| error.to_string())?;
    let bytes = read(&path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != resource.sha256 {
        return Err(format!(
            "Texture {} changed; reimport the intended file",
            resource.path
        ));
    }
    height_image(decode(&bytes, &resource.kind)?)
}

/// The bundled original grayscale texture follows exactly the imported height
/// path: red-channel heights, floating-point mip averaging, physical gradients.
pub(crate) fn builtin_height(kind: cad_core::appearance::BuiltinBump) -> Result<Image, String> {
    use cad_core::appearance::BuiltinBump;
    let bytes: &[u8] = match kind {
        BuiltinBump::None => return Err("The finish has no built-in height map".into()),
        BuiltinBump::Brushed => include_bytes!("../../../assets/appearance/brushed-height-v1.png"),
        BuiltinBump::Blasted => include_bytes!("../../../assets/appearance/blasted-height-v1.png"),
        BuiltinBump::Machined => {
            include_bytes!("../../../assets/appearance/machined-height-v1.png")
        }
        BuiltinBump::OrangePeel => {
            include_bytes!("../../../assets/appearance/orange-peel-height-v1.png")
        }
        BuiltinBump::Molded => include_bytes!("../../../assets/appearance/molded-height-v1.png"),
        BuiltinBump::Wood => include_bytes!("../../../assets/appearance/wood-height-v1.png"),
    };
    height_image(decode(bytes, "png")?)
}

fn height_image(source: DynamicImage) -> Result<Image, String> {
    let (mut width, mut height) = (source.width(), source.height());
    let original = (width, height);
    let red_u8 = |pixels: &[u8], channels: usize| {
        pixels
            .chunks_exact(channels)
            .map(|pixel| f32::from(pixel[0]) / 255.)
            .collect::<Vec<_>>()
    };
    let red_u16 = |pixels: &[u16], channels: usize| {
        pixels
            .chunks_exact(channels)
            .map(|pixel| f32::from(pixel[0]) / 65535.)
            .collect::<Vec<_>>()
    };
    let mut values = match &source {
        DynamicImage::ImageLuma8(pixels) => red_u8(pixels.as_raw(), 1),
        DynamicImage::ImageLumaA8(pixels) => red_u8(pixels.as_raw(), 2),
        DynamicImage::ImageRgb8(pixels) => red_u8(pixels.as_raw(), 3),
        DynamicImage::ImageRgba8(pixels) => red_u8(pixels.as_raw(), 4),
        DynamicImage::ImageLuma16(pixels) => red_u16(pixels.as_raw(), 1),
        DynamicImage::ImageLumaA16(pixels) => red_u16(pixels.as_raw(), 2),
        DynamicImage::ImageRgb16(pixels) => red_u16(pixels.as_raw(), 3),
        DynamicImage::ImageRgba16(pixels) => red_u16(pixels.as_raw(), 4),
        DynamicImage::ImageRgb32F(pixels) => pixels
            .as_raw()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|pixel| pixel[0])
            .collect(),
        DynamicImage::ImageRgba32F(pixels) => pixels
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[0])
            .collect(),
        _ => return Err("Unsupported grayscale relief pixel format".into()),
    };
    drop(source);
    if values
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err("Grayscale relief values must be finite normalized heights from 0 to 1".into());
    }
    let mut offset = 0;
    let mut levels = 1;
    while width > 1 || height > 1 {
        let next_width = (width / 2).max(1);
        let next_height = (height / 2).max(1);
        let mut next = vec![0.; next_width as usize * next_height as usize];
        for y in 0..next_height {
            for x in 0..next_width {
                let (x0, x1) = (x * width / next_width, (x + 1) * width / next_width);
                let (y0, y1) = (y * height / next_height, (y + 1) * height / next_height);
                let mut sum = 0_f64;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        sum +=
                            f64::from(values[offset + sy as usize * width as usize + sx as usize]);
                    }
                }
                next[y as usize * next_width as usize + x as usize] =
                    (sum / f64::from((x1 - x0) * (y1 - y0))) as f32;
            }
        }
        offset = values.len();
        values.extend(next);
        width = next_width;
        height = next_height;
        levels += 1;
    }
    let mut data = Vec::with_capacity(values.len() * 2);
    for value in values {
        data.extend_from_slice(&half::f16::from_f32(value).to_le_bytes());
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: original.0,
            height: original.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::R16Float,
        RenderAssetUsages::default(),
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Ok(image)
}
