//! Lazy authored visual catalog and portable checksum-pinned finish libraries.
//! This contract carries appearance only, never density, strength or CAD geometry.
use crate::{CadError, Document, Id, Resource, Result, appearance::*};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
    sync::OnceLock,
};

pub const LIBRARY_VERSION: u32 = 1;
pub const LIBRARY_FORMAT: &str = "tcad.surface-finishes";
pub const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TOTAL_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
const SOURCE: &str = "TCAD authored visual approximation, WTFPL. PBR values are artistic display parameters; color is uncalibrated sRGB.";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub finish: Finish,
    pub category: String,
    pub tags: Vec<String>,
    pub source: String,
}
fn entry(mut finish: Finish, category: &str, tags: &[&str], description: &str) -> CatalogEntry {
    let tags = tags.iter().map(|tag| (*tag).into()).collect::<Vec<_>>();
    finish.metadata = Some(FinishMetadata {
        category: category.into(),
        tags: tags.clone(),
        description: description.into(),
        source: SOURCE.into(),
        catalog_id: Some(finish.id.clone()),
    });
    CatalogEntry {
        finish,
        category: category.into(),
        tags,
        source: SOURCE.into(),
    }
}
fn optical(
    id: &str,
    name: &str,
    color: [f64; 3],
    metallic: f64,
    roughness: f64,
    bump: BuiltinBump,
) -> Finish {
    Finish {
        id: format!("finish-{id}"),
        name: name.into(),
        base_color: color,
        metallic,
        roughness,
        clearcoat: 0.,
        clearcoat_roughness: 0.25,
        builtin_bump: bump,
        anisotropy: if matches!(bump, BuiltinBump::Brushed | BuiltinBump::Machined) {
            0.5
        } else {
            0.
        },
        height_mm: match bump {
            BuiltinBump::None => 0.,
            BuiltinBump::Brushed => 0.012,
            BuiltinBump::Blasted => 0.008,
            BuiltinBump::Machined => 0.018,
            BuiltinBump::OrangePeel => 0.045,
            BuiltinBump::Molded => 0.015,
            BuiltinBump::Wood => 0.035,
        },
        mapping: TextureMapping {
            scale_mm: match bump {
                BuiltinBump::Machined => [12., 12.],
                BuiltinBump::OrangePeel | BuiltinBump::Molded => [8., 8.],
                BuiltinBump::Wood => [100., 100.],
                _ => [25., 25.],
            },
            ..TextureMapping::default()
        },
        ..Finish::default()
    }
}
/// Catalog entries stay outside document defaults. Browsing never changes a
/// source fingerprint, pins textures, or inserts unused finishes into a project.
pub fn catalog() -> &'static [CatalogEntry] {
    static CATALOG: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut catalog = default_finishes().into_values().map(|finish| {
            let category = if finish.id == "painted-ral1015" { "Paint and powder coat" } else { "Aluminum" };
            CatalogEntry { finish, category: category.into(), tags: vec!["built-in".into()], source: SOURCE.into() }
        }).collect::<Vec<_>>();
        for (id, name, category, color, metallic, roughness, bump, tags) in [
            ("aluminum-machined", "Aluminum · Machined", "Aluminum", [0.77,0.79,0.81], 1.,0.28,BuiltinBump::Machined, vec!["metal","bare","machined"]),
            ("aluminum-brushed", "Aluminum · Satin Brushed", "Aluminum", [0.78,0.80,0.82], 1.,0.36,BuiltinBump::Brushed, vec!["metal","bare","satin"]),
            ("aluminum-polished", "Aluminum · Polished", "Aluminum", [0.82,0.84,0.86], 1.,0.10,BuiltinBump::None, vec!["metal","bare","polished"]),
            ("aluminum-blasted", "Aluminum · Bead Blasted", "Aluminum", [0.70,0.72,0.74], 1.,0.54,BuiltinBump::Blasted, vec!["metal","bare","matte"]),
            ("steel-mill", "Steel · Mill Finish", "Steel and stainless", [0.30,0.32,0.34], 1.,0.61,BuiltinBump::Blasted, vec!["metal","bare","mill"]),
            ("steel-machined", "Steel · Machined", "Steel and stainless", [0.56,0.58,0.61], 1.,0.30,BuiltinBump::Machined, vec!["metal","bare","machined"]),
            ("steel-polished", "Steel · Polished", "Steel and stainless", [0.65,0.67,0.70], 1.,0.12,BuiltinBump::None, vec!["metal","bare","polished"]),
            ("stainless-brushed", "Stainless Steel · Brushed", "Steel and stainless", [0.65,0.67,0.69], 1.,0.32,BuiltinBump::Brushed, vec!["metal","stainless","satin"]),
            ("stainless-polished", "Stainless Steel · Mirror Polished", "Steel and stainless", [0.70,0.72,0.74], 1.,0.07,BuiltinBump::None, vec!["metal","stainless","polished"]),
            ("stainless-blasted", "Stainless Steel · Bead Blasted", "Steel and stainless", [0.55,0.57,0.59], 1.,0.58,BuiltinBump::Blasted, vec!["metal","stainless","matte"]),
            ("steel-black-oxide", "Steel · Black Oxide", "Steel and stainless", [0.06,0.07,0.08], 0.85,0.43,BuiltinBump::Blasted, vec!["metal","coated","black"]),
            ("cast-iron", "Cast Iron · As Cast", "Other metals", [0.22,0.23,0.24], 0.85,0.76,BuiltinBump::Molded, vec!["metal","cast","matte"]),
            ("copper-satin", "Copper · Satin", "Other metals", [0.85,0.50,0.35], 1.,0.32,BuiltinBump::Brushed, vec!["metal","bare","satin"]),
            ("copper-polished", "Copper · Polished", "Other metals", [0.95,0.63,0.47], 1.,0.10,BuiltinBump::None, vec!["metal","bare","polished"]),
            ("copper-patina", "Copper · Patina Approximation", "Other metals", [0.24,0.45,0.34], 0.12,0.77,BuiltinBump::Blasted, vec!["metal","patina","green"]),
            ("brass-satin", "Brass · Satin", "Other metals", [0.84,0.69,0.36], 1.,0.32,BuiltinBump::Brushed, vec!["metal","bare","satin"]),
            ("brass-polished", "Brass · Polished", "Other metals", [0.91,0.78,0.45], 1.,0.10,BuiltinBump::None, vec!["metal","bare","polished"]),
            ("bronze", "Bronze · Brushed", "Other metals", [0.68,0.46,0.26], 1.,0.38,BuiltinBump::Brushed, vec!["metal","bare","satin"]),
            ("titanium", "Titanium · Satin", "Other metals", [0.53,0.55,0.59], 1.,0.42,BuiltinBump::Brushed, vec!["metal","bare","satin"]),
            ("galvanized", "Zinc · Galvanized Approximation", "Other metals", [0.67,0.70,0.73], 0.90,0.46,BuiltinBump::Blasted, vec!["metal","coated","zinc"]),
            ("chrome", "Chrome · Polished", "Other metals", [0.74,0.76,0.79], 1.,0.06,BuiltinBump::None, vec!["metal","coated","polished"]),
            ("abs-black", "ABS · Black Molded", "Plastics and elastomers", [0.055,0.058,0.063], 0.,0.43,BuiltinBump::Molded, vec!["plastic","black","molded"]),
            ("abs-white", "ABS · White Molded", "Plastics and elastomers", [0.89,0.90,0.91], 0.,0.43,BuiltinBump::Molded, vec!["plastic","white","molded"]),
            ("nylon-natural", "Nylon · Natural", "Plastics and elastomers", [0.86,0.83,0.72], 0.,0.49,BuiltinBump::Molded, vec!["plastic","natural","molded"]),
            ("acetal-black", "Acetal · Black Machined", "Plastics and elastomers", [0.045,0.05,0.06], 0.,0.29,BuiltinBump::Machined, vec!["plastic","black","machined"]),
            ("ptfe-natural", "PTFE · Natural", "Plastics and elastomers", [0.94,0.94,0.93], 0.,0.64,BuiltinBump::None, vec!["plastic","natural","matte"]),
            ("rubber-black", "Rubber · Black", "Plastics and elastomers", [0.025,0.027,0.03], 0.,0.86,BuiltinBump::Molded, vec!["elastomer","black","matte"]),
            ("silicone-gray", "Silicone · Gray", "Plastics and elastomers", [0.42,0.44,0.46], 0.,0.72,BuiltinBump::Molded, vec!["elastomer","gray","matte"]),
            ("ceramic-white", "Ceramic · Glazed White", "Other surfaces", [0.94,0.93,0.91], 0.,0.16,BuiltinBump::None, vec!["ceramic","white","glazed"]),
            ("wood-maple", "Wood · Maple Approximation", "Other surfaces", [0.76,0.57,0.35], 0.,0.56,BuiltinBump::Wood, vec!["wood","grain","maple"]),
            ("wood-walnut", "Wood · Walnut Approximation", "Other surfaces", [0.31,0.18,0.10], 0.,0.52,BuiltinBump::Wood, vec!["wood","grain","walnut"]),
        ] {
            catalog.push(entry(optical(id, name, color, metallic, roughness, bump), category, &tags, "Authored visual finish. Imported texture maps can replace the bundled grayscale relief; this preset has no effect on mass or engineering properties."));
        }
        for (id, name, color) in [
            ("black", "Black", [0.055,0.06,0.07]), ("blue", "Blue", [0.07,0.24,0.55]),
            ("red", "Red", [0.57,0.10,0.09]), ("gold", "Gold", [0.68,0.50,0.17]),
            ("bronze", "Bronze", [0.36,0.23,0.13]), ("violet", "Violet", [0.36,0.12,0.47]),
        ] {
            let mut finish = optical(&format!("anodized-{id}"), &format!("Aluminum · {name} Anodized"), color, 1., 0.33, BuiltinBump::Brushed);
            finish.clearcoat = 0.18;
            catalog.push(entry(finish, "Aluminum", &["metal","anodized","colored"], "Uncalibrated dyed anodized appearance with grayscale grain. Anodize thickness, alloy and process specifications belong in engineering properties."));
        }
        for (code, name, color) in [
            ("1003", "Signal Yellow", [0.96,0.73,0.04]), ("1015", "Light Ivory", [0.90,0.84,0.68]),
            ("2004", "Pure Orange", [0.92,0.32,0.08]), ("3000", "Flame Red", [0.66,0.11,0.14]),
            ("3020", "Traffic Red", [0.78,0.10,0.10]), ("5010", "Gentian Blue", [0.05,0.23,0.43]),
            ("5015", "Sky Blue", [0.13,0.48,0.69]), ("6005", "Moss Green", [0.12,0.24,0.19]),
            ("7016", "Anthracite Gray", [0.22,0.25,0.27]), ("7035", "Light Gray", [0.78,0.79,0.77]),
            ("9005", "Jet Black", [0.04,0.04,0.04]), ("9010", "Pure White", [0.94,0.93,0.89]),
            ("9016", "Traffic White", [0.96,0.96,0.94]),
        ] {
            for (sheen, roughness, bump, coat) in [("Matte",0.64,BuiltinBump::OrangePeel,0.03),("Satin",0.39,BuiltinBump::OrangePeel,0.12),("Gloss",0.18,BuiltinBump::None,0.35)] {
                let mut finish = optical(&format!("paint-ral{code}-{}", sheen.to_lowercase()), &format!("Paint · RAL {code} {name} · {sheen} (Approx.)"), color, 0., roughness, bump);
                finish.clearcoat = coat;
                catalog.push(entry(finish, "Paint and powder coat", &["paint","powder coat",code,sheen], "RAL names identify a common color family only. RGB swatches are authored display approximations, not official licensed RAL color data or calibrated manufacturing specifications."));
            }
        }
        catalog.sort_by(|a,b| (&a.category,&a.finish.name).cmp(&(&b.category,&b.finish.name)));
        catalog
    })
}
pub fn instantiate(doc: &Document, catalog_id: &str) -> Result<Finish> {
    let mut finish = catalog()
        .iter()
        .find(|entry| entry.finish.id == catalog_id)
        .ok_or_else(|| {
            CadError::Invalid("The selected visual finish catalog entry does not exist".into())
        })?
        .finish
        .clone();
    if doc
        .finishes
        .get(&finish.id)
        .is_some_and(|existing| existing != &finish)
    {
        finish.id = crate::id();
    }
    validate_finish(doc, &finish)?;
    Ok(finish)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortableTexture {
    pub resource: Resource,
    /// Exact encoded image bytes, pinned by the resource checksum. No file URI.
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package {
    pub format: String,
    pub version: u32,
    pub finishes: Vec<Finish>,
    #[serde(default)]
    pub textures: Vec<PortableTexture>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn bounded_read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum as u64 {
        return Err(invalid(
            "Visual library textures must be nonempty regular files within the 64 MiB texture bound",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("Texture grew beyond the library size bound"));
    }
    Ok(bytes)
}
pub fn export(doc: &Document, root: &Path, ids: &[Id]) -> Result<Package> {
    let mut finishes = vec![];
    let mut resources = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for id in ids {
        if !selected.insert(id) {
            continue;
        }
        let finish = doc
            .finishes
            .get(id)
            .ok_or_else(|| invalid("An exported finish no longer exists"))?;
        validate_finish(doc, finish)?;
        resources.extend(finish.textures.resources().cloned());
        finishes.push(finish.clone());
    }
    let mut textures = vec![];
    let mut total = 0_usize;
    for id in resources {
        let resource = doc
            .resources
            .get(&id)
            .ok_or_else(|| invalid("An exported finish texture is missing"))?;
        let bytes = bounded_read(
            &crate::persistence::safe_join(root, &resource.path)?,
            MAX_TEXTURE_BYTES,
        )?;
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL_TEXTURE_BYTES {
            return Err(invalid(
                "A portable finish library supports at most 64 MiB of encoded texture bytes; export smaller finish groups",
            ));
        }
        textures.push(PortableTexture {
            resource: resource.clone(),
            bytes,
        });
    }
    let package = Package {
        format: LIBRARY_FORMAT.into(),
        version: LIBRARY_VERSION,
        finishes,
        textures,
    };
    validate_package(&package)?;
    Ok(package)
}
fn validate_package(package: &Package) -> Result<()> {
    if package.format != LIBRARY_FORMAT || package.version != LIBRARY_VERSION {
        return Err(invalid("Unsupported visual finish library format/version"));
    }
    if package.finishes.is_empty() || package.finishes.len() > 1024 || package.textures.len() > 1024
    {
        return Err(invalid(
            "A portable finish library requires 1–1024 finishes and at most 1024 textures",
        ));
    }
    let mut document = Document::empty("Visual finish library validation");
    let mut total = 0_usize;
    for texture in &package.textures {
        total = total.saturating_add(texture.bytes.len());
        if texture.bytes.is_empty()
            || texture.bytes.len() > MAX_TEXTURE_BYTES
            || total > MAX_TOTAL_TEXTURE_BYTES
        {
            return Err(invalid(
                "A finish library supports at most 64 MiB of encoded texture bytes",
            ));
        }
        if format!("{:x}", Sha256::digest(&texture.bytes)) != texture.resource.sha256 {
            return Err(invalid(
                "Portable texture bytes do not match their pinned SHA-256 checksum",
            ));
        }
        if document
            .resources
            .insert(texture.resource.id.clone(), texture.resource.clone())
            .is_some()
        {
            return Err(invalid("Duplicate portable texture identity"));
        }
    }
    let mut ids = BTreeSet::new();
    let mut used = BTreeSet::new();
    for finish in &package.finishes {
        if !ids.insert(&finish.id) {
            return Err(invalid("Duplicate finish identity in portable library"));
        }
        validate_finish(&document, finish)?;
        used.extend(finish.textures.resources().cloned());
    }
    if used.len() != package.textures.len()
        || package
            .textures
            .iter()
            .any(|texture| !used.contains(&texture.resource.id))
    {
        return Err(invalid(
            "Portable libraries may include only textures referenced by their finishes",
        ));
    }
    Ok(())
}
pub fn encode(package: &Package) -> Result<Vec<u8>> {
    validate_package(package)?;
    let bytes = serde_json::to_vec(package)?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(invalid(
            "Portable finish library exceeds the 256 MiB JSON size bound",
        ));
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Package> {
    if bytes.is_empty() || bytes.len() > MAX_PACKAGE_BYTES {
        return Err(invalid(
            "Finish library must be nonempty JSON no larger than 256 MiB",
        ));
    }
    let package: Package = serde_json::from_slice(bytes)?;
    validate_package(&package)?;
    Ok(package)
}
/// Stage immutable assets and install finishes atomically in the supplied source
/// clone. UI Apply still waits for current bounded image decoding readiness.
pub fn install(doc: &mut Document, root: &Path, package: &Package) -> Result<Vec<Id>> {
    validate_package(package)?;
    let mut staged = doc.clone();
    let mut mapping = BTreeMap::new();
    for texture in &package.textures {
        let resource = &texture.resource;
        let existing = staged
            .resources
            .values()
            .find(|existing| {
                existing.sha256 == resource.sha256
                    && existing.kind.eq_ignore_ascii_case(&resource.kind)
                    && crate::persistence::safe_join(root, &existing.path)
                        .ok()
                        .and_then(|path| bounded_read(&path, MAX_TEXTURE_BYTES).ok())
                        .is_some_and(|bytes| {
                            format!("{:x}", Sha256::digest(bytes)) == resource.sha256
                        })
            })
            .map(|resource| resource.id.clone());
        let id = if let Some(id) = existing {
            id
        } else {
            crate::persistence::import_resource_bytes(
                &mut staged,
                root,
                Path::new(&format!("texture.{}", resource.kind)),
                &texture.bytes,
            )?
        };
        mapping.insert(resource.id.clone(), id);
    }
    let mut installed = vec![];
    for source in &package.finishes {
        let mut finish = source.clone();
        for slot in [
            &mut finish.textures.base_color,
            &mut finish.textures.normal,
            &mut finish.textures.roughness,
            &mut finish.textures.metallic,
            &mut finish.textures.occlusion,
            &mut finish.textures.height,
        ] {
            if let Some(id) = slot.as_mut() {
                *id = mapping
                    .get(id)
                    .ok_or_else(|| invalid("Imported finish texture is missing"))?
                    .clone();
            }
        }
        if let Some(existing) = staged
            .finishes
            .values()
            .find(|existing| **existing == finish)
        {
            installed.push(existing.id.clone());
            continue;
        }
        if staged.finishes.contains_key(&finish.id) {
            finish.id = crate::id();
        }
        validate_finish(&staged, &finish)?;
        installed.push(finish.id.clone());
        staged.finishes.insert(finish.id.clone(), finish);
    }
    staged.validate()?;
    *doc = staged;
    Ok(installed)
}
