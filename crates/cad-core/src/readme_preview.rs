//! Generated repository previews. The native document remains authoritative.
use crate::{CadError, Document, Result, persistence::atomic_write, protocol::Evaluation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, path::Path};

pub const CONTRACT_VERSION: u32 = 1;
pub const START: &str = "<!-- tcad:model-preview:start -->";
pub const END: &str = "<!-- tcad:model-preview:end -->";
const INLINE_LIMIT: usize = 180_000;
const MAX_TRIANGLES: usize = 180_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub version: u32,
    pub source_sha256: String,
    pub source_revision: u64,
    pub backend: String,
    pub accuracy: crate::tolerance::Accuracy,
    pub triangles: usize,
    pub inline: bool,
    pub stl_sha256: String,
    pub units: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub metadata: Metadata,
    pub stl: Vec<u8>,
    pub png: Vec<u8>,
    pub markdown: String,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("README preview: {message}"))
}
type Triangle = [[f64; 3]; 3];
fn normal(t: &Triangle) -> [f64; 3] {
    let a = nalgebra::Vector3::from(t[1]) - nalgebra::Vector3::from(t[0]);
    let b = nalgebra::Vector3::from(t[2]) - nalgebra::Vector3::from(t[0]);
    let n = a.cross(&b);
    if n.norm_squared() > 0. {
        n.normalize().into()
    } else {
        [0.; 3]
    }
}
/// Requires the full, current evaluation; never drops arbitrary triangles to meet a size limit.
pub fn build(doc: &Document, evaluation: &Evaluation) -> Result<Preview> {
    if !evaluation.valid || evaluation.identity.source != doc.fingerprint() {
        return Err(invalid(
            "A valid evaluation of this exact saved model is required",
        ));
    }
    let mut triangles = Vec::new();
    for instance in evaluation.instances.iter().chain(
        evaluation
            .assembly_welds
            .values()
            .map(|weld| &weld.placement),
    ) {
        let key = if instance.mesh_key.is_empty() {
            &instance.definition
        } else {
            &instance.mesh_key
        };
        let mesh = evaluation
            .meshes
            .get(key)
            .ok_or_else(|| invalid("A placed component has no evaluated mesh"))?;
        if mesh.indices.len() % 3 != 0 {
            return Err(invalid("The evaluated mesh has incomplete triangles"));
        }
        if triangles.len() + mesh.indices.len() / 3 > MAX_TRIANGLES {
            return Err(invalid(
                "This model exceeds the GitHub preview size budget. Increase the display chord tolerance and generate the preview again",
            ));
        }
        for indices in mesh.indices.as_chunks::<3>().0 {
            let mut triangle = [[0.; 3]; 3];
            for (destination, index) in triangle.iter_mut().zip(indices) {
                let point = mesh
                    .positions
                    .get(*index as usize)
                    .ok_or_else(|| invalid("Mesh index is out of range"))?;
                *destination = std::array::from_fn(|row| {
                    instance.matrix[row][0] * point[0]
                        + instance.matrix[row][1] * point[1]
                        + instance.matrix[row][2] * point[2]
                        + instance.matrix[row][3]
                });
                if destination
                    .iter()
                    .any(|x| !x.is_finite() || x.abs() > f32::MAX as f64)
                {
                    return Err(invalid("Mesh coordinates are not finite STL coordinates"));
                }
            }
            triangles.push(triangle);
        }
    }
    if triangles.is_empty() {
        return Err(invalid("The model has no evaluated surfaces to show"));
    }
    let mut ascii = String::from("solid tcad_preview\n");
    for triangle in &triangles {
        let n = normal(triangle);
        let _ = writeln!(
            ascii,
            "facet normal {:.7e} {:.7e} {:.7e}\nouter loop",
            n[0], n[1], n[2]
        );
        for p in triangle {
            let _ = writeln!(ascii, "vertex {:.7e} {:.7e} {:.7e}", p[0], p[1], p[2]);
        }
        ascii.push_str("endloop\nendfacet\n");
        if ascii.len() > INLINE_LIMIT {
            break;
        }
    }
    ascii.push_str("endsolid tcad_preview\n");
    let inline = ascii.len() <= INLINE_LIMIT;
    let stl = if inline {
        ascii.as_bytes().to_vec()
    } else {
        let mut bytes = vec![0; 80];
        let header = b"TCAD generated preview; coordinates in millimeters";
        bytes[..header.len()].copy_from_slice(header);
        bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for t in &triangles {
            for coordinate in normal(t).into_iter().chain(t.iter().flatten().copied()) {
                bytes.extend_from_slice(&(coordinate as f32).to_le_bytes());
            }
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        bytes
    };
    let metadata = Metadata {
        version: CONTRACT_VERSION,
        source_sha256: doc.fingerprint(),
        source_revision: doc.revision,
        backend: evaluation.identity.backend.clone(),
        accuracy: evaluation.accuracy,
        triangles: triangles.len(),
        inline,
        stl_sha256: format!("{:x}", Sha256::digest(&stl)),
        units: "mm".into(),
    };
    let mut markdown = format!(
        "{START}\n\n### Model preview\n\nGenerated from saved model `{}`. Coordinates are in millimeters.\n\n",
        &metadata.source_sha256[..16]
    );
    if inline {
        markdown.push_str("Drag to rotate. Scroll to zoom on GitHub.\n\n```stl\n");
        markdown.push_str(&ascii);
        markdown.push_str("```\n\n");
    } else {
        markdown.push_str("[![Model preview](previews/model.png)](previews/model.stl)\n\nOpen the preview to rotate and zoom in GitHub's STL viewer.\n\n");
    }
    markdown.push_str("[Download model preview](previews/model.stl) · [Preview source record](previews/model-preview.json)\n\nThis is a generated display mesh. Use the native TCAD files for editing and manufacturing.\n\n");
    markdown.push_str(END);
    markdown.push('\n');
    Ok(Preview {
        metadata,
        stl,
        png: thumbnail(&triangles)?,
        markdown,
    })
}
fn thumbnail(triangles: &[Triangle]) -> Result<Vec<u8>> {
    const WIDTH: usize = 800;
    const HEIGHT: usize = 560;
    let project = |p: [f64; 3]| {
        [
            std::f64::consts::FRAC_1_SQRT_2 * (p[0] - p[1]),
            (p[0] + p[1] - 2. * p[2]) / 6_f64.sqrt(),
            (p[0] + p[1] + p[2]) / 3_f64.sqrt(),
        ]
    };
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in triangles.iter().flatten().copied().map(project) {
        for i in 0..2 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let scale = (720. / (max[0] - min[0]).max(1e-9)).min(480. / (max[1] - min[1]).max(1e-9));
    let mut pixels = vec![0u8; WIDTH * HEIGHT * 3];
    for rgb in pixels.as_chunks_mut::<3>().0 {
        rgb.copy_from_slice(&[241, 242, 244]);
    }
    let mut depth = vec![f64::NEG_INFINITY; WIDTH * HEIGHT];
    let area = |a: [f64; 3], b: [f64; 3], x: f64, y: f64| {
        (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
    };
    for t in triangles {
        let projected = t.map(|p| {
            let q = project(p);
            [
                400. + (q[0] - (min[0] + max[0]) * 0.5) * scale,
                280. + (q[1] - (min[1] + max[1]) * 0.5) * scale,
                q[2],
            ]
        });
        let denominator = area(projected[0], projected[1], projected[2][0], projected[2][1]);
        if denominator.abs() < 1e-12 {
            continue;
        }
        let bounds = |axis: usize, limit: usize| {
            let lo = projected
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .clamp(0., (limit - 1) as f64) as usize;
            let hi = projected
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .clamp(0., (limit - 1) as f64) as usize;
            (lo, hi)
        };
        let (x0, x1) = bounds(0, WIDTH);
        let (y0, y1) = bounds(1, HEIGHT);
        let n = normal(t);
        let light = (0.55 + 0.4 * (n[0] * 0.2 + n[1] * 0.4 + n[2] * 0.9)).clamp(0.18, 0.95);
        let shade = (light * 220.) as u8;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let u =
                    area(projected[1], projected[2], x as f64 + 0.5, y as f64 + 0.5) / denominator;
                let v =
                    area(projected[2], projected[0], x as f64 + 0.5, y as f64 + 0.5) / denominator;
                let w = 1. - u - v;
                if u < -1e-9 || v < -1e-9 || w < -1e-9 {
                    continue;
                }
                let z = u * projected[0][2] + v * projected[1][2] + w * projected[2][2];
                let index = y * WIDTH + x;
                if z > depth[index] {
                    depth[index] = z;
                    pixels[index * 3..index * 3 + 3].copy_from_slice(&[
                        shade,
                        shade,
                        shade.saturating_add(8),
                    ]);
                }
            }
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, WIDTH as u32, HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| invalid(&format!("Thumbnail encoding failed: {error}")))?;
        writer
            .write_image_data(&pixels)
            .map_err(|error| invalid(&format!("Thumbnail encoding failed: {error}")))?;
    }
    Ok(bytes)
}
/// Replace only our marked section. Malformed or duplicate markers require user repair.
pub fn update_markdown(existing: &str, preview: &Preview) -> Result<String> {
    update_markdown_block(existing, &preview.markdown)
}
/// Share the same marked-section replacement rule with repository publication.
/// Callers supply a generated, complete TCAD preview block, never user prose.
pub fn update_markdown_block(existing: &str, markdown: &str) -> Result<String> {
    let starts: Vec<_> = existing.match_indices(START).collect();
    let ends: Vec<_> = existing.match_indices(END).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(format!("{}\n\n{}", existing.trim_end(), markdown)),
        ([(start, _)], [(end, _)]) if end > start => Ok(format!(
            "{}{}{}",
            &existing[..*start],
            markdown.trim_end(),
            &existing[end + END.len()..]
        )),
        _ => Err(invalid(
            "README has incomplete or repeated TCAD preview markers; fix the marked section before updating it",
        )),
    }
}
/// Reject changed source; resources are derived and never alter authoritative definitions.
pub fn write(project: &Path, doc: &Document, preview: &Preview) -> Result<()> {
    use crate::persistence::{FileStore, ProjectStore};
    if crate::release::is_release_source(project) {
        return Err(invalid(
            "Save a working copy before updating a released model's README",
        ));
    }
    if preview.metadata.version != CONTRACT_VERSION
        || preview.metadata.source_sha256 != doc.fingerprint()
    {
        return Err(invalid("Preview source no longer matches the model"));
    }
    let root = crate::persistence::project_root(project);
    let saved = FileStore.load(project)?;
    if saved.fingerprint() != doc.fingerprint() {
        return Err(invalid(
            "Save the current model before updating its README preview",
        ));
    }
    let readme_path = root.join("README.md");
    for path in [
        &readme_path,
        &root.join("previews"),
        &root.join("previews/model.stl"),
        &root.join("previews/model.png"),
        &root.join("previews/model-preview.json"),
    ] {
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(invalid("Preview paths must not be symbolic links"));
        }
    }
    let existing = match std::fs::read_to_string(&readme_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            format!("# {}\n", doc.name.replace(['\r', '\n'], " "))
        }
        Err(error) => return Err(error.into()),
    };
    let updated = update_markdown(&existing, preview)?;
    atomic_write(&root.join("previews/model.stl"), &preview.stl)?;
    atomic_write(&root.join("previews/model.png"), &preview.png)?;
    atomic_write(
        &root.join("previews/model-preview.json"),
        &serde_json::to_vec_pretty(&preview.metadata)?,
    )?;
    atomic_write(&readme_path, updated.as_bytes())
}
