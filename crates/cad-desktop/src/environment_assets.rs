//! Bounded local equirectangular HDR lighting assets; source bytes remain immutable.
use cad_core::{CadError, Resource};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
pub const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_RGB_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DECODER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 1024 * 1024;
const MAX_RADIANCE: f32 = 1e12;

pub(crate) struct Panorama {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<f32>,
    pub maximum: f32,
}
impl Panorama {
    /// Bilinear linear-radiance sampling. Longitude wraps; poles clamp.
    pub fn sample(&self, direction: bevy::math::Vec3) -> bevy::math::Vec3 {
        let u = direction.z.atan2(direction.x) / std::f32::consts::TAU + 0.5;
        let v = direction.y.clamp(-1., 1.).acos() / std::f32::consts::PI;
        let x = u * self.width as f32 - 0.5;
        let y = (v * self.height as f32 - 0.5).clamp(0., self.height as f32 - 1.);
        let x0 = x.floor() as i32;
        let y0 = y.floor() as u32;
        let tx = x - x.floor();
        let ty = y - y.floor();
        let pixel = |x: i32, y: u32| {
            let index = (y.min(self.height - 1) as usize * self.width as usize
                + x.rem_euclid(self.width as i32) as usize)
                * 3;
            bevy::math::Vec3::new(
                self.pixels[index],
                self.pixels[index + 1],
                self.pixels[index + 2],
            )
        };
        pixel(x0, y0)
            .lerp(pixel(x0 + 1, y0), tx)
            .lerp(pixel(x0, y0 + 1).lerp(pixel(x0 + 1, y0 + 1), tx), ty)
    }
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("Cannot read environment: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect environment: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err("Environment must be a nonempty regular file no larger than 128 MiB".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read environment: {error}"))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Environment grew beyond the 128 MiB file bound".into());
    }
    Ok(bytes)
}
fn format(kind: &str) -> Result<ImageFormat, String> {
    match kind.to_ascii_lowercase().as_str() {
        "hdr" => Ok(ImageFormat::Hdr),
        "exr" => Ok(ImageFormat::OpenExr),
        _ => Err("Choose a Radiance .hdr or flat RGB .exr environment".into()),
    }
}
fn dimensions(width: u32, height: u32) -> Result<(), String> {
    if height == 0
        || width != height.saturating_mul(2)
        || width > 8192
        || height > 4096
        || u64::from(width) * u64::from(height) * 12 > MAX_RGB_BYTES
    {
        return Err("Environment must be a 2:1 equirectangular panorama within 8192×4096 and 128 MiB of linear RGB pixels; 4096×2048 is supported".into());
    }
    Ok(())
}
fn cstring<'a>(bytes: &'a [u8], offset: &mut usize, max: usize) -> Result<&'a [u8], String> {
    let rest = bytes.get(*offset..).ok_or("Truncated EXR header")?;
    let length = rest
        .iter()
        .take(max + 1)
        .position(|byte| *byte == 0)
        .ok_or("EXR header string exceeds its bound")?;
    let value = &rest[..length];
    *offset += length + 1;
    Ok(value)
}
fn box2i(bytes: &[u8]) -> Result<[i32; 4], String> {
    if bytes.len() != 16 {
        return Err("EXR window metadata must contain four bounded integers".into());
    }
    Ok(std::array::from_fn(|index| {
        i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    }))
}
/// Preflight the supported single-part scanline profile before codec metadata
/// allocation. Equal windows bound internal EXR buffers as well as output pixels.
fn exr_profile(bytes: &[u8]) -> Result<(), String> {
    if bytes.get(..4) != Some(&[0x76, 0x2f, 0x31, 0x01]) {
        return Err("EXR magic does not match the file extension".into());
    }
    let version = u32::from_le_bytes(
        bytes
            .get(4..8)
            .ok_or("Truncated EXR version")?
            .try_into()
            .unwrap(),
    );
    if version & 0xff != 2 || version & !0x4ff != 0 {
        return Err("Environment EXR supports version 2 single-part non-deep scanline RGB; flatten tiled, multipart or deep files before import".into());
    }
    let mut offset = 8;
    let mut names = BTreeSet::new();
    let mut data_window = None;
    let mut display_window = None;
    let mut rgb = false;
    let mut compression = false;
    loop {
        if offset > MAX_HEADER_BYTES || names.len() > 128 {
            return Err("EXR environment header exceeds its bounded attribute profile".into());
        }
        let name = cstring(
            bytes,
            &mut offset,
            if version & 0x400 != 0 { 255 } else { 31 },
        )?;
        if name.is_empty() {
            break;
        }
        if !names.insert(name.to_vec()) {
            return Err("Duplicate EXR header attribute".into());
        }
        let kind = cstring(
            bytes,
            &mut offset,
            if version & 0x400 != 0 { 255 } else { 31 },
        )?;
        let length = u32::from_le_bytes(
            bytes
                .get(offset..offset + 4)
                .ok_or("Truncated EXR attribute size")?
                .try_into()
                .unwrap(),
        ) as usize;
        offset += 4;
        if length > MAX_HEADER_BYTES || offset.saturating_add(length) > MAX_HEADER_BYTES {
            return Err("EXR attribute exceeds the 1 MiB header bound".into());
        }
        let value = bytes
            .get(offset..offset + length)
            .ok_or("Truncated EXR attribute")?;
        offset += length;
        match name {
            b"dataWindow" if kind == b"box2i" => data_window = Some(box2i(value)?),
            b"displayWindow" if kind == b"box2i" => display_window = Some(box2i(value)?),
            b"compression" if kind == b"compression" && value.len() == 1 && value[0] <= 7 => {
                compression = true
            }
            b"channels" if kind == b"chlist" => {
                let mut channels = BTreeSet::new();
                let mut cursor = 0;
                loop {
                    let channel = cstring(value, &mut cursor, 31)?;
                    if channel.is_empty() {
                        break;
                    }
                    if !matches!(channel, b"R" | b"G" | b"B" | b"A")
                        || !channels.insert(channel.to_vec())
                    {
                        return Err(
                            "EXR environment needs flat R/G/B channels and optional A only".into(),
                        );
                    }
                    let definition = value
                        .get(cursor..cursor + 16)
                        .ok_or("Truncated EXR channel definition")?;
                    cursor += 16;
                    let number = |index| {
                        i32::from_le_bytes(definition[index..index + 4].try_into().unwrap())
                    };
                    if !matches!(number(0), 1 | 2) || number(8) != 1 || number(12) != 1 {
                        return Err("EXR environment channels must be HALF/FLOAT RGB without chroma subsampling".into());
                    }
                }
                if cursor != value.len() {
                    return Err("Unexpected EXR channel metadata tail".into());
                }
                rgb = [b"R".as_slice(), b"G", b"B"]
                    .iter()
                    .all(|channel| channels.contains(*channel));
            }
            b"chromaticities" if kind == b"chromaticities" => {
                if value.len() != 32 {
                    return Err("Invalid EXR RGB chromaticities".into());
                }
                let expected = [0.64_f32, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290];
                if expected.iter().enumerate().any(|(index, expected)| {
                    let actual =
                        f32::from_le_bytes(value[index * 4..index * 4 + 4].try_into().unwrap());
                    !actual.is_finite() || (actual - expected).abs() > 0.002
                }) {
                    return Err("Convert EXR chromaticities to linear sRGB/Rec.709 before importing this environment".into());
                }
            }
            _ => {}
        }
    }
    let window = display_window.ok_or("EXR display window is missing")?;
    if data_window != Some(window) || !rgb || !compression {
        return Err("EXR environment needs matching full data/display windows, RGB channels and supported standard compression".into());
    }
    let width = i64::from(window[2]) - i64::from(window[0]) + 1;
    let height = i64::from(window[3]) - i64::from(window[1]) + 1;
    dimensions(
        u32::try_from(width).map_err(|_| "Invalid EXR window width")?,
        u32::try_from(height).map_err(|_| "Invalid EXR window height")?,
    )
}
fn reader<'a>(bytes: &'a [u8], kind: &str) -> Result<ImageReader<Cursor<&'a [u8]>>, String> {
    let expected = format(kind)?;
    if image::guess_format(bytes)
        .map_err(|error| format!("Cannot identify environment: {error}"))?
        != expected
    {
        return Err("Environment contents do not match their .hdr/.exr extension".into());
    }
    if expected == ImageFormat::OpenExr {
        exr_profile(bytes)?;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), expected);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(MAX_DECODER_BYTES);
    reader.limits(limits);
    Ok(reader)
}
pub(crate) fn validate_file(path: &Path) -> cad_core::Result<Vec<u8>> {
    let kind = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    (|| -> Result<Vec<u8>, String> {
        let bytes = read(path)?;
        let (width, height) = reader(&bytes, kind)?
            .into_dimensions()
            .map_err(|error| format!("Cannot inspect environment: {error}"))?;
        dimensions(width, height)?;
        Ok(bytes)
    })()
    .map_err(|message| CadError::Invalid(format!("Environment: {message}")))
}
pub(crate) fn load(root: &Path, resource: &Resource) -> Result<Panorama, String> {
    let path = cad_core::persistence::safe_join(root, &resource.path)
        .map_err(|error| error.to_string())?;
    let bytes = read(&path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != resource.sha256 {
        return Err("Environment resource changed; reimport its intended bytes".into());
    }
    let (width, height) = reader(&bytes, &resource.kind)?
        .into_dimensions()
        .map_err(|error| format!("Cannot inspect environment: {error}"))?;
    dimensions(width, height)?;
    let image = if format(&resource.kind)? == ImageFormat::OpenExr {
        // Force RGB output so optional alpha does not allocate a second RGBA-sized
        // panorama; equal-window preflight bounds the decoder's internal RGB copy.
        let mut decoder = image::codecs::openexr::OpenExrDecoder::with_alpha_preference(
            Cursor::new(&bytes),
            Some(false),
        )
        .map_err(|error| format!("Cannot create environment decoder: {error}"))?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(MAX_DECODER_BYTES);
        limits.reserve(decoder.total_bytes()).map_err(|error| {
            format!("Environment output exceeds the decoder allocation budget: {error}")
        })?;
        decoder
            .set_limits(limits)
            .map_err(|error| format!("Environment decoder limits: {error}"))?;
        DynamicImage::from_decoder(decoder)
            .map_err(|error| format!("Cannot decode environment: {error}"))?
            .into_rgb32f()
    } else {
        reader(&bytes, &resource.kind)?
            .decode()
            .map_err(|error| format!("Cannot decode environment: {error}"))?
            .into_rgb32f()
    };
    if image.width() != width || image.height() != height {
        return Err("Environment dimensions changed while decoding".into());
    }
    let values = image.into_raw();
    let mut maximum = 0_f32;
    for value in &values {
        if !value.is_finite() || !(0.0..=MAX_RADIANCE).contains(value) {
            return Err(
                "Environment RGB radiance must be finite, nonnegative and at most 1e12 per channel"
                    .into(),
            );
        }
        maximum = maximum.max(*value);
    }
    let pixels = values;
    Ok(Panorama {
        width,
        height,
        pixels,
        maximum,
    })
}
