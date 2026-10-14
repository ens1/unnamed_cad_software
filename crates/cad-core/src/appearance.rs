//! Renderer-independent surface finishes and illumination. Engineering materials remain separate.
use crate::{CadError, Document, Id, PlacedPart, Result, presentation::SemanticElement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 4;
pub const DEFAULT_FINISH: &str = "clear-anodized-aluminum";
pub const TEXTURE_KINDS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "webp"];

pub fn default_finish() -> Id {
    DEFAULT_FINISH.into()
}
pub fn is_default_finish(id: &Id) -> bool {
    id == DEFAULT_FINISH
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MappingMode {
    #[default]
    Triplanar,
    Planar,
}
/// Dimensions and offsets are in the owning part's local millimetres, independent of tessellation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextureMapping {
    pub mode: MappingMode,
    pub scale_mm: [f64; 2],
    pub rotation_deg: f64,
    pub offset_mm: [f64; 2],
}
impl Default for TextureMapping {
    fn default() -> Self {
        Self {
            mode: MappingMode::Triplanar,
            scale_mm: [25., 25.],
            rotation_deg: 0.,
            offset_mm: [0., 0.],
        }
    }
}
/// Immutable project Resource identities. Base color is sRGB; normal/scalar maps are linear.
/// Scalar maps use the red channel; normals use the OpenGL (+Y) convention.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextureMaps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion: Option<Id>,
    /// Grayscale height for lighting relief; never changes native CAD surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Id>,
}
impl TextureMaps {
    pub fn resources(&self) -> impl Iterator<Item = &Id> {
        [
            &self.base_color,
            &self.normal,
            &self.roughness,
            &self.metallic,
            &self.occlusion,
            &self.height,
        ]
        .into_iter()
        .filter_map(Option::as_ref)
    }
}
/// Built-in grayscale height image used when no imported height resource is assigned.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinBump {
    None,
    #[default]
    Brushed,
    Blasted,
    Machined,
    OrangePeel,
    Molded,
    Wood,
}
impl BuiltinBump {
    pub const MAPS: [Self; 6] = [
        Self::Brushed,
        Self::Blasted,
        Self::Machined,
        Self::OrangePeel,
        Self::Molded,
        Self::Wood,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Brushed => "Brushed grain",
            Self::Blasted => "Blasted microtexture",
            Self::Machined => "Machined tool marks",
            Self::OrangePeel => "Coating orange peel",
            Self::Molded => "Molded texture",
            Self::Wood => "Wood grain",
        }
    }
}
/// Optional human-facing library metadata. Absence preserves legacy serialized
/// finish identities; these notes never establish engineering material properties.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FinishMetadata {
    pub category: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub description: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<Id>,
}
/// Explicit authored manufacturing requirements, separate from optical shader
/// values. An optical library preset does not establish a coating specification.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ManufacturingFinish {
    pub process: String,
    pub specification: String,
    pub designation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coating_thickness_um: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_roughness_ra_um: Option<f64>,
    pub notes: String,
}
impl ManufacturingFinish {
    pub fn validate(&self) -> Result<()> {
        if [&self.process, &self.specification, &self.designation]
            .iter()
            .any(|value| value.len() > 512 || value.chars().any(char::is_control))
            || self.notes.len() > 4096
            || self
                .notes
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(invalid(
                "Manufacturing finish requirements exceed their supported text bounds",
            ));
        }
        if [self.coating_thickness_um, self.surface_roughness_ra_um]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || !(0.0..=1_000_000.0).contains(&value))
        {
            return Err(invalid(
                "Coating thickness and specified surface Ra must be finite, nonnegative micrometre values",
            ));
        }
        Ok(())
    }
}
/// Contract v2 adds a built-in grayscale bump selection and directional reflection controls.
/// Older serialized finishes use these fields' default values without changing their existing
/// tint, texture resources or other explicitly stored surface properties.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Finish {
    pub id: Id,
    pub name: String,
    /// Display tint in sRGB. A base-color image multiplies this tint.
    pub base_color: [f64; 3],
    pub metallic: f64,
    pub roughness: f64,
    pub clearcoat: f64,
    pub clearcoat_roughness: f64,
    pub normal_strength: f64,
    pub height_mm: f64,
    /// Imported height textures take precedence over this built-in grayscale height image.
    pub builtin_bump: BuiltinBump,
    /// Directional reflection strength; zero gives isotropic reflections.
    pub anisotropy: f64,
    pub textures: TextureMaps,
    pub mapping: TextureMapping,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FinishMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturing: Option<ManufacturingFinish>,
}
impl Default for Finish {
    fn default() -> Self {
        Self {
            id: default_finish(),
            name: "Aluminum · Clear Anodized".into(),
            base_color: [0.78, 0.80, 0.82],
            metallic: 1.,
            roughness: 0.32,
            clearcoat: 0.18,
            clearcoat_roughness: 0.25,
            normal_strength: 0.25,
            height_mm: 0.006,
            builtin_bump: BuiltinBump::Brushed,
            anisotropy: 0.45,
            textures: TextureMaps::default(),
            mapping: TextureMapping::default(),
            metadata: None,
            manufacturing: None,
        }
    }
}
pub fn default_finishes() -> BTreeMap<Id, Finish> {
    [
        Finish::default(),
        Finish {
            id: "bare-aluminum".into(),
            name: "Aluminum · Bare".into(),
            base_color: [0.80, 0.81, 0.83],
            roughness: 0.22,
            clearcoat: 0.,
            anisotropy: 0.65,
            ..Finish::default()
        },
        Finish {
            id: "green-anodized-aluminum".into(),
            name: "Aluminum · Green Anodized".into(),
            base_color: [0.10, 0.40, 0.20],
            roughness: 0.30,
            ..Finish::default()
        },
        Finish {
            id: "painted-ral1015".into(),
            name: "Paint · RAL 1015 (Display Approximation)".into(),
            base_color: [0.90, 0.84, 0.68],
            metallic: 0.,
            roughness: 0.40,
            clearcoat: 0.10,
            builtin_bump: BuiltinBump::None,
            anisotropy: 0.,
            ..Finish::default()
        },
    ]
    .into_iter()
    .map(|finish| (finish.id.clone(), finish))
    .collect()
}
pub fn is_default_finishes(finishes: &BTreeMap<Id, Finish>) -> bool {
    *finishes == default_finishes()
}
/// Studio illumination multipliers, environment yaw and photographic exposure in stops.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Lighting {
    /// Immutable local HDR/EXR equirectangular radiance. None keeps the authored studio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Id>,
    pub environment_rotation_deg: f64,
    pub exposure: f64,
    pub intensity: f64,
    pub key_intensity: f64,
    pub fill_intensity: f64,
    pub shadows: bool,
}
impl Default for Lighting {
    fn default() -> Self {
        Self {
            environment: None,
            environment_rotation_deg: 0.,
            exposure: 0.,
            intensity: 1.,
            key_intensity: 1.,
            fill_intensity: 1.,
            shadows: true,
        }
    }
}
pub fn is_default_lighting(lighting: &Lighting) -> bool {
    *lighting == Lighting::default()
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !matches!(id, "." | "..")
        && !id
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
pub fn validate_finish(doc: &Document, finish: &Finish) -> Result<()> {
    if let Some(manufacturing) = &finish.manufacturing {
        manufacturing.validate()?;
    }
    if !valid_id(&finish.id) || finish.name.trim().is_empty() || finish.name.len() > 256 {
        return Err(invalid(
            "Finishes require a portable ID and a name of at most 256 bytes",
        ));
    }
    if let Some(metadata) = &finish.metadata
        && (metadata.category.len() > 128
            || metadata.description.len() > 4096
            || metadata.source.len() > 2048
            || metadata.tags.len() > 64
            || metadata.tags.iter().any(|tag| tag.len() > 128)
            || metadata.catalog_id.as_ref().is_some_and(|id| !valid_id(id))
            || [&metadata.category, &metadata.description, &metadata.source]
                .into_iter()
                .chain(metadata.tags.iter())
                .any(|text| {
                    text.chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
                }))
    {
        return Err(invalid(
            "Finish library metadata exceeds its portable text bounds",
        ));
    }
    if finish
        .base_color
        .iter()
        .chain([
            &finish.metallic,
            &finish.roughness,
            &finish.clearcoat,
            &finish.clearcoat_roughness,
            &finish.anisotropy,
        ])
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || !finish.normal_strength.is_finite()
        || !(0.0..=4.0).contains(&finish.normal_strength)
        || !finish.height_mm.is_finite()
        || !(0.0..=10.0).contains(&finish.height_mm)
    {
        return Err(invalid(
            "Finish color/PBR and anisotropy values must be 0–1; normal strength 0–4 and relief depth 0–10 mm",
        ));
    }
    let mapping = &finish.mapping;
    if mapping
        .scale_mm
        .iter()
        .any(|v| !v.is_finite() || !(0.001..=1_000_000.).contains(v))
        || !mapping.rotation_deg.is_finite()
        || mapping.rotation_deg.abs() > 1_000_000.
        || mapping
            .offset_mm
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        return Err(invalid(
            "Texture tile sizes must be 0.001–1000000 mm; mapping values must be finite and bounded",
        ));
    }
    for resource_id in finish.textures.resources() {
        let resource = doc.resources.get(resource_id).ok_or_else(|| {
            invalid(format!(
                "Finish {} references missing texture resource {resource_id}",
                finish.name
            ))
        })?;
        if resource.id != *resource_id
            || !valid_id(resource_id)
            || !TEXTURE_KINDS.contains(&resource.kind.to_ascii_lowercase().as_str())
            || resource.sha256.len() != 64
            || !resource.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid(format!(
                "Texture {} must be a pinned PNG, JPEG, TIFF or WebP resource",
                resource.path
            )));
        }
        crate::persistence::safe_join(std::path::Path::new("."), &resource.path)?;
        if resource.path.is_empty()
            || resource.path.len() > 2048
            || resource
                .path
                .chars()
                .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        {
            return Err(invalid("Texture resources require a portable file path"));
        }
    }
    Ok(())
}
pub fn validate_lighting(lighting: &Lighting) -> Result<()> {
    if !lighting.environment_rotation_deg.is_finite()
        || lighting.environment_rotation_deg.abs() > 1_000_000.
        || !lighting.exposure.is_finite()
        || !(-8.0..=8.0).contains(&lighting.exposure)
        || [
            lighting.intensity,
            lighting.key_intensity,
            lighting.fill_intensity,
        ]
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=10.0).contains(v))
    {
        return Err(invalid(
            "Lighting requires bounded finite values: exposure −8–8 stops, intensities 0–10",
        ));
    }
    Ok(())
}
pub fn validate_environment(doc: &Document, environment: Option<&Id>) -> Result<()> {
    let Some(id) = environment else {
        return Ok(());
    };
    let resource = doc
        .resources
        .get(id)
        .ok_or_else(|| invalid("The selected HDR environment resource is missing"))?;
    if resource.id != *id
        || !valid_id(id)
        || !matches!(resource.kind.to_ascii_lowercase().as_str(), "hdr" | "exr")
        || resource.sha256.len() != 64
        || !resource.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || resource.path.is_empty()
        || resource.path.len() > 2048
        || resource
            .path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
    {
        return Err(invalid(
            "HDR environments require a portable checksum-pinned .hdr/.exr resource",
        ));
    }
    crate::persistence::safe_join(std::path::Path::new("."), &resource.path)?;
    Ok(())
}
pub fn validate(doc: &Document) -> Result<()> {
    if !doc.finishes.contains_key(DEFAULT_FINISH) {
        return Err(invalid(
            "The default clear anodized aluminum finish is required for new parts",
        ));
    }
    if doc.finishes.len() > 4096 {
        return Err(invalid("A document may contain at most 4096 finishes"));
    }
    for (key, finish) in &doc.finishes {
        if key != &finish.id {
            return Err(invalid("Finish map key and stable identity must agree"));
        }
        validate_finish(doc, finish)?;
    }
    for part in doc.parts.values() {
        if !doc.finishes.contains_key(&part.finish) {
            return Err(invalid(format!(
                "Part {} references missing finish {}",
                part.name, part.finish
            )));
        }
        validate_face_finishes(doc, part)?;
    }
    for assembly in doc.assemblies.values() {
        for instance in assembly.instances.values() {
            if let Some(finish) = &instance.finish
                && !doc.finishes.contains_key(finish)
            {
                return Err(invalid(format!(
                    "Component {} references missing finish {finish}",
                    instance.name
                )));
            }
        }
    }
    validate_environment(doc, doc.lighting.environment.as_ref())?;
    validate_lighting(&doc.lighting)
}
pub fn resolve_finish<'a>(doc: &'a Document, placed: &PlacedPart) -> Result<&'a Finish> {
    // Geometry may come from the last valid evaluation while a visual draft is pending.
    // Resolve current source assignments, including patterned/nested configured components,
    // rather than using the finish cached alongside that older geometry.
    let mut context = crate::configuration::effective(doc)?;
    let mut definition = doc.root.clone();
    let mut override_id: Option<Id> = None;
    let mut current_path = true;
    for id in &placed.path {
        let Some(instance) = context
            .assemblies
            .get(&definition)
            .and_then(|a| a.instances.get(id))
            .cloned()
        else {
            current_path = false;
            break;
        };
        if let Some(finish) = &instance.finish {
            override_id = Some(finish.clone());
        }
        let configuration = crate::configuration::selected(
            &context,
            &instance.definition,
            instance.configuration.as_ref(),
        )
        .cloned();
        context = crate::configuration::scope(
            doc,
            &context,
            &instance.definition,
            configuration.as_deref(),
        )?;
        definition = instance.definition;
    }
    let source = if current_path && definition == placed.definition {
        &context
    } else {
        doc
    };
    let part = source
        .parts
        .get(&placed.definition)
        .ok_or_else(|| invalid("The appearance part is missing"))?;
    let finish = override_id
        .as_deref()
        .or_else(|| {
            (!current_path)
                .then_some(placed.finish.as_deref())
                .flatten()
        })
        .unwrap_or(&part.finish);
    doc.finishes
        .get(finish)
        .ok_or_else(|| invalid(format!("Missing finish {finish}")))
}
/// Resolve current visual assignments in one configured hierarchy traversal, suitable for
/// render adapters drawing many bodies from each occurrence's cached geometry.
pub fn resolved_finishes(doc: &Document) -> Result<BTreeMap<Vec<Id>, Id>> {
    doc.placed_parts()?
        .into_iter()
        .map(|part| {
            let finish = part
                .finish
                .ok_or_else(|| invalid("The evaluated appearance assignment is missing"))?;
            if !doc.finishes.contains_key(&finish) {
                return Err(invalid(format!("Missing finish {finish}")));
            }
            Ok((part.path, finish))
        })
        .collect()
}
pub fn delete_reason(doc: &Document, id: &str) -> Option<&'static str> {
    if crate::library::finish_is_pinned(doc, id) {
        return Some(
            "This finish belongs to an immutable component snapshot; duplicate its pinned source before editing",
        );
    }
    if id == DEFAULT_FINISH {
        return Some("The default clear anodized aluminum finish is required for new parts");
    }
    if id == doc.settings.default_new_part_finish {
        return Some("Choose another default new-part finish in model settings before deleting it");
    }
    if doc.parts.values().any(|p| p.finish == id)
        || crate::display_states::finish_is_used(doc, id)
        || doc
            .parts
            .values()
            .any(|p| p.face_finishes.iter().any(|face| face.finish == id))
        || doc.assemblies.values().any(|a| {
            a.instances
                .values()
                .any(|i| i.finish.as_deref() == Some(id))
        })
        || doc.parts.values().any(|p| {
            p.configuration_details
                .values()
                .any(|c| c.overrides.finish.as_deref() == Some(id))
        })
    {
        return Some(
            "Reassign the finish's parts, faces, components and configurations before deleting it",
        );
    }
    None
}
pub fn delete_finish(doc: &mut Document, id: &str) -> Result<()> {
    if let Some(reason) = delete_reason(doc, id) {
        return Err(invalid(reason));
    }
    if doc.finishes.remove(id).is_none() {
        return Err(invalid("The finish no longer exists"));
    }
    Ok(())
}
/// A part-owned face finish follows construction provenance. Signatures are
/// repair hints only: proximity, face order and mesh indices never reattach it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FaceFinish {
    pub id: Id,
    pub body: crate::NodeRef,
    pub face: crate::TopologyRef,
    pub finish: Id,
}

#[derive(Clone)]
enum FaceIdentity {
    Key(std::sync::Arc<str>),
    Invalid(String),
    Unsupported(String),
}
impl FaceIdentity {
    fn result(&self) -> Result<std::sync::Arc<str>> {
        match self {
            Self::Key(key) => Ok(key.clone()),
            Self::Invalid(message) => Err(CadError::Invalid(message.clone())),
            Self::Unsupported(message) => Err(CadError::Unsupported(message.clone())),
        }
    }
}

/// Disposable appearance index over authored source identity, independent of
/// mesh order and geometric signatures. Legacy source trees and closure tables
/// normalize once per distinct role. Shared keys avoid copying a large closure
/// into each render/index entry; aggregate bounds cover the complete lookup.
pub struct FaceFinishIdentityCache<'a> {
    keys: BTreeMap<(&'a crate::NodeRef, &'a str, &'a str), FaceIdentity>,
    input_left: usize,
    output_left: usize,
}
impl Default for FaceFinishIdentityCache<'_> {
    fn default() -> Self {
        Self {
            keys: BTreeMap::new(),
            input_left: 64 * 1024 * 1024,
            output_left: 64 * 1024 * 1024,
        }
    }
}
impl<'a> FaceFinishIdentityCache<'a> {
    pub fn key(&mut self, reference: &'a crate::TopologyRef) -> Result<std::sync::Arc<str>> {
        let identity = (
            &reference.producer,
            reference.topology.as_str(),
            reference.role.as_str(),
        );
        if let Some(cached) = self.keys.get(&identity) {
            return cached.result();
        }
        if self.keys.len() >= 65_536 {
            return Err(CadError::Unsupported(
                "Face finish source identity index exceeds 65536 distinct references".into(),
            ));
        }
        let value = if let Some(remaining) = self.input_left.checked_sub(reference.role.len()) {
            self.input_left = remaining;
            match crate::provenance::canonical_key(reference) {
                Ok(key) => match self.output_left.checked_sub(key.len()) {
                    Some(remaining) => {
                        self.output_left = remaining;
                        FaceIdentity::Key(key.into())
                    }
                    None => FaceIdentity::Unsupported(
                        "Face finish canonical identity index exceeds 64 MiB".into(),
                    ),
                },
                Err(CadError::Unsupported(message)) => FaceIdentity::Unsupported(message),
                Err(error) => FaceIdentity::Invalid(error.to_string()),
            }
        } else {
            FaceIdentity::Unsupported("Face finish source identity input exceeds 64 MiB".into())
        };
        let result = value.result();
        self.keys.insert(identity, value);
        result
    }
}
fn validate_face_finishes(doc: &Document, part: &crate::Part) -> Result<()> {
    if part.face_finishes.len() > 16_384 {
        return Err(invalid(
            "A part may contain at most 16384 face finish assignments",
        ));
    }
    let mut identities = std::collections::BTreeSet::new();
    let mut targets = std::collections::BTreeSet::new();
    let mut canonical = FaceFinishIdentityCache::default();
    for assignment in &part.face_finishes {
        if !valid_id(&assignment.id)
            || !identities.insert(&assignment.id)
            || assignment.body.definition != part.id
            || assignment.face.producer.definition != part.id
            || assignment.face.topology != "face"
            || assignment.face.role.is_empty()
            || assignment.face.role.len() > 1_048_576
            || !doc.finishes.contains_key(&assignment.finish)
            || !targets.insert((&assignment.body, canonical.key(&assignment.face)?))
        {
            return Err(invalid(format!(
                "Part {} has an invalid, duplicate or foreign face finish assignment",
                part.name
            )));
        }
    }
    Ok(())
}
pub fn set_face_finish(doc: &mut Document, definition: &str, assignment: FaceFinish) -> Result<()> {
    let part = doc
        .parts
        .get(definition)
        .ok_or_else(|| invalid("The face finish part no longer exists"))?;
    let mut candidate = part.clone();
    // Reassigning the same source face retains one semantic override, even if
    // the UI provided a new identity for this intentional edit.
    let mut retained = Vec::new();
    for existing in candidate.face_finishes.drain(..) {
        if existing.id != assignment.id
            && !(existing.body == assignment.body
                && crate::provenance::equivalent(&existing.face, &assignment.face)?)
        {
            retained.push(existing);
        }
    }
    candidate.face_finishes = retained;
    candidate.face_finishes.push(assignment);
    validate_face_finishes(doc, &candidate)?;
    doc.parts.insert(definition.into(), candidate);
    Ok(())
}
pub fn delete_face_finish(doc: &mut Document, definition: &str, assignment: &str) -> Result<()> {
    let part = doc
        .parts
        .get_mut(definition)
        .ok_or_else(|| invalid("The face finish part no longer exists"))?;
    let length = part.face_finishes.len();
    part.face_finishes.retain(|face| face.id != assignment);
    if part.face_finishes.len() == length {
        return Err(invalid(
            "The selected face finish assignment no longer exists",
        ));
    }
    Ok(())
}
/// Disposable exact resolution against evaluated native face metadata. Multiple
/// native matches are a split/ambiguous source and intentionally remain unresolved.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedFaceFinish {
    pub assignment: Id,
    pub finish: Id,
    pub body: crate::NodeRef,
    pub face: crate::TopologyRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceFinishDiagnostic {
    pub assignment: Id,
    pub state: String,
    pub matches: usize,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FaceFinishResolution {
    pub resolved: Vec<ResolvedFaceFinish>,
    pub diagnostics: Vec<FaceFinishDiagnostic>,
}
fn body_ancestors(part: &crate::Part, output: &crate::NodeRef) -> std::collections::BTreeSet<Id> {
    let mut pending = vec![output.node.clone()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node.clone()) {
            continue;
        }
        if let Some(feature) = part.features.iter().find(|feature| feature.id == node) {
            pending.extend(
                feature
                    .operation
                    .dependencies()
                    .into_iter()
                    .filter(|dependency| dependency.definition == part.id)
                    .map(|dependency| dependency.node),
            );
        }
    }
    seen
}
pub fn resolve_face_finishes(
    part: &crate::Part,
    meshes: &[&crate::protocol::MeshData],
) -> FaceFinishResolution {
    let mut resolution = FaceFinishResolution::default();
    if part.face_finishes.is_empty() {
        return resolution;
    }
    // Canonical authored identity is independent of whether a saved role uses
    // the legacy source tree or the new bounded source closure representation.
    let mut canonical = FaceFinishIdentityCache::default();
    let mut faces = BTreeMap::<std::sync::Arc<str>, Vec<&crate::protocol::FaceRange>>::new();
    let mut ancestors = BTreeMap::new();
    let mut native_errors = BTreeMap::new();
    for face in meshes.iter().flat_map(|mesh| &mesh.faces) {
        if face.reference.topology != "face" {
            continue;
        }
        let Some(body) = &face.body else { continue };
        if body.definition != part.id {
            continue;
        }
        ancestors
            .entry(body.clone())
            .or_insert_with(|| body_ancestors(part, body));
        match canonical.key(&face.reference) {
            Ok(key) => {
                faces.entry(key).or_default().push(face);
            }
            Err(error) => {
                // An unknown identity could otherwise conceal an ambiguous
                // descendant. Keep all assignments for this producer/body
                // recoverable rather than claiming a unique match.
                native_errors
                    .entry((body.clone(), face.reference.producer.clone()))
                    .or_insert_with(|| error.to_string());
            }
        }
    }
    let mut resolved_targets = BTreeMap::new();
    let mut resolved_keys = BTreeMap::new();
    for assignment in &part.face_finishes {
        let key = match canonical.key(&assignment.face) {
            Ok(key) => key,
            Err(error) => {
                resolution.diagnostics.push(FaceFinishDiagnostic {
                    assignment: assignment.id.clone(), state: "invalid-reference".into(), matches: 0,
                    message: format!("The saved face identity cannot be normalized: {error}. The part finish is shown; repair or remove this assignment."),
                });
                continue;
            }
        };
        if let Some(error) = native_errors.iter().find_map(|((body, producer), error)| {
            (producer == &assignment.face.producer
                && ancestors
                    .get(body)
                    .is_some_and(|sources| sources.contains(&assignment.body.node)))
            .then_some(error)
        }) {
            resolution.diagnostics.push(FaceFinishDiagnostic {
                assignment: assignment.id.clone(), state: "invalid-native-reference".into(), matches: 0,
                message: format!("Current native face identities cannot be resolved: {error}. The part finish is shown; the saved assignment is retained."),
            });
            continue;
        }
        let matches = faces
            .get(&key)
            .into_iter()
            .flatten()
            .copied()
            .filter(|face| {
                assignment.body.definition == part.id
                    && face
                        .body
                        .as_ref()
                        .and_then(|body| ancestors.get(body))
                        .is_some_and(|sources| sources.contains(&assignment.body.node))
            })
            .collect::<Vec<_>>();
        if let [face] = matches.as_slice()
            && let Some(body) = &face.body
        {
            let target = (body.clone(), key);
            *resolved_targets.entry(target.clone()).or_insert(0_usize) += 1;
            resolved_keys.insert(assignment.id.clone(), target);
            resolution.resolved.push(ResolvedFaceFinish {
                assignment: assignment.id.clone(),
                finish: assignment.finish.clone(),
                body: body.clone(),
                face: face.reference.clone(),
            });
        } else {
            resolution.diagnostics.push(FaceFinishDiagnostic {
                assignment: assignment.id.clone(),
                state: if matches.is_empty() { "missing" } else { "ambiguous" }.into(),
                matches: matches.len(),
                message: if matches.is_empty() {
                    "The assigned face is absent from this body/configuration; the part finish is shown. Reselect a face to repair the assignment."
                } else {
                    "The assigned native face has multiple descendants; the part finish is shown. Select the intended current face to repair the assignment."
                }.into(),
            });
        }
    }
    // Distinct authored body ancestors can converge on the same canonical
    // native face. Neither assignment wins; both remain explicitly repairable.
    resolution.resolved.retain(|resolved| {
        let key = &resolved_keys[&resolved.assignment];
        if resolved_targets[key] <= 1 { return true; }
        resolution.diagnostics.push(FaceFinishDiagnostic {
            assignment: resolved.assignment.clone(), state: "conflict".into(), matches: 1,
            message: "Multiple source assignments converge on this native face; keep one assignment. The part finish is shown.".into(),
        });
        false
    });
    resolution
}

fn field_element(
    id: String,
    label: &str,
    state: &str,
    relationship: &str,
    value: serde_json::Value,
    mut data: serde_json::Value,
) -> SemanticElement {
    data["contract"] = "tcad.appearance".into();
    data["version"] = CONTRACT_VERSION.into();
    data["json"] = value.clone();
    data["editable"] = true.into();
    data["accessibility_label"] = label.into();
    SemanticElement {
        id,
        role: if data["setter"] == "appearance-lighting" {
            "lighting-field"
        } else {
            "appearance-field"
        }
        .into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: state.into(),
        commands: vec!["appearance.edit".into()],
        relationships: vec![relationship.into()],
        data,
    }
}
pub fn semantic(doc: &Document, active: Option<&str>, state: &str) -> Vec<SemanticElement> {
    let mut out = vec![SemanticElement {
        id: "surface-finishes".into(),
        role: "appearance-library".into(),
        label: "Surface Finishes".into(),
        value: Some(doc.finishes.len().to_string()),
        state: state.into(),
        commands: vec!["appearances".into(), "appearance.new".into()],
        relationships: vec![doc.id.clone()],
        data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"engineering_material":false,"default_finish":DEFAULT_FINISH}),
    }];
    for finish in doc.finishes.values() {
        let identity = format!("finish:{}", finish.id);
        let source = serde_json::to_value(finish).unwrap_or_default();
        out.push(SemanticElement {
            id: identity.clone(), role: "surface-finish".into(), label: finish.name.clone(), value: None,
            state: state.into(), commands: vec!["appearance.edit".into(), "appearance.duplicate".into(), "appearance.delete".into()],
            relationships: finish.textures.resources().cloned().collect(),
            data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"finish":finish.id,"source":finish,"engineering_material":false,"color_space":"srgb","scalar_channel":"red","normal_convention":"opengl-positive-y","height_unit":"mm","height_changes_geometry":false,"height_source_precedence":["textures.height","builtin_bump"],"actions":[{"command":"appearance.delete","label":"Delete Finish","enabled":delete_reason(doc,&finish.id).is_none(),"disabled_reason":delete_reason(doc,&finish.id),"arguments":{"finish":finish.id}}]}),
        });
        for (field, label, minimum, maximum, unit, choices) in [
            ("/name", "Finish Name", None, None, None, vec![]),
            (
                "/base_color/0",
                "Tint · Red",
                Some(0.),
                Some(1.),
                None,
                vec![],
            ),
            (
                "/base_color/1",
                "Tint · Green",
                Some(0.),
                Some(1.),
                None,
                vec![],
            ),
            (
                "/base_color/2",
                "Tint · Blue",
                Some(0.),
                Some(1.),
                None,
                vec![],
            ),
            ("/metallic", "Metallic", Some(0.), Some(1.), None, vec![]),
            ("/roughness", "Roughness", Some(0.), Some(1.), None, vec![]),
            ("/clearcoat", "Clearcoat", Some(0.), Some(1.), None, vec![]),
            (
                "/clearcoat_roughness",
                "Clearcoat Roughness",
                Some(0.),
                Some(1.),
                None,
                vec![],
            ),
            (
                "/builtin_bump",
                "Built-in Bump Map",
                None,
                None,
                None,
                vec![
                    "none",
                    "brushed",
                    "blasted",
                    "machined",
                    "orange_peel",
                    "molded",
                    "wood",
                ],
            ),
            (
                "/anisotropy",
                "Directional Reflections",
                Some(0.),
                Some(1.),
                None,
                vec![],
            ),
            (
                "/normal_strength",
                "Normal Strength",
                Some(0.),
                Some(4.),
                None,
                vec![],
            ),
            (
                "/height_mm",
                "Grayscale Relief Depth",
                Some(0.),
                Some(10.),
                Some("mm"),
                vec![],
            ),
            (
                "/mapping/mode",
                "Texture Mapping",
                None,
                None,
                None,
                vec!["triplanar", "planar"],
            ),
            (
                "/mapping/scale_mm/0",
                "Texture Tile · U",
                Some(0.001),
                Some(1_000_000.),
                Some("mm"),
                vec![],
            ),
            (
                "/mapping/scale_mm/1",
                "Texture Tile · V",
                Some(0.001),
                Some(1_000_000.),
                Some("mm"),
                vec![],
            ),
            (
                "/mapping/rotation_deg",
                "Texture Direction",
                Some(-1_000_000.),
                Some(1_000_000.),
                Some("deg"),
                vec![],
            ),
            (
                "/mapping/offset_mm/0",
                "Texture Offset · U",
                Some(-1_000_000.),
                Some(1_000_000.),
                Some("mm"),
                vec![],
            ),
            (
                "/mapping/offset_mm/1",
                "Texture Offset · V",
                Some(-1_000_000.),
                Some(1_000_000.),
                Some("mm"),
                vec![],
            ),
        ] {
            let value = source.pointer(field).cloned().unwrap_or_default();
            out.push(field_element(format!("{identity}::{field}"), label, state, &identity, value.clone(),
                serde_json::json!({"setter":"appearance-finish","finish":finish.id,"field":field,"value_type":if !choices.is_empty(){"choice"}else if value.is_number(){"number"}else{"string"},"minimum":minimum,"maximum":maximum,"unit":unit,"choices":choices,"edit_command":"appearance.edit"})));
        }
        for (slot, label, resource) in [
            (
                "base_color",
                "Base Color Texture",
                &finish.textures.base_color,
            ),
            ("normal", "Normal Texture", &finish.textures.normal),
            ("roughness", "Roughness Texture", &finish.textures.roughness),
            ("metallic", "Metallic Texture", &finish.textures.metallic),
            ("occlusion", "Occlusion Texture", &finish.textures.occlusion),
            (
                "height",
                "Grayscale Relief Texture",
                &finish.textures.height,
            ),
        ] {
            let mut element = field_element(
                format!("{identity}::/textures/{slot}"),
                label,
                state,
                &identity,
                serde_json::json!(resource),
                serde_json::json!({"setter":"appearance-finish","finish":finish.id,"field":format!("/textures/{slot}"),"slot":slot,"value_type":"reference","choices":std::iter::once(serde_json::json!({"value":null,"label":"None"})).chain(doc.resources.values().filter(|r|TEXTURE_KINDS.contains(&r.kind.to_ascii_lowercase().as_str())).map(|r|serde_json::json!({"value":r.id,"label":r.path}))).collect::<Vec<_>>(),"texture":resource.as_ref().and_then(|id|doc.resources.get(id)),"nullable":true,"edit_command":"appearance.edit"}),
            );
            element.commands.extend([
                "appearance.texture.import".into(),
                "appearance.texture.clear".into(),
            ]);
            if let Some(resource) = resource {
                element.relationships.push(resource.clone());
            }
            out.push(element);
        }
    }
    let choices: Vec<_> = doc
        .finishes
        .values()
        .map(|f| serde_json::json!({"value":f.id,"label":f.name}))
        .collect();
    for part in doc
        .parts
        .values()
        .filter(|part| active.is_none_or(|id| id == part.id))
    {
        out.push(SemanticElement {
            id: format!("{}::finish", part.id), role: "appearance-assignment".into(), label: "Base Part Finish".into(),
            value: Some(part.finish.clone()), state: state.into(), commands: vec!["appearances".into(), "appearance.assign-part".into()],
            relationships: vec![part.id.clone(), format!("finish:{}", part.finish)],
            data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"setter":"appearance-part","definition":part.id,"finish":part.finish,"json":part.finish,"value_type":"choice","choices":choices,"editable":true,"edit_command":"appearance.assign-part","engineering_material":part.material}),
        });
        for assignment in &part.face_finishes {
            out.push(SemanticElement {
                id: format!("{}::face-finish:{}", part.id, assignment.id),
                role: "face-appearance-assignment".into(),
                label: "Face Finish".into(), value: Some(assignment.finish.clone()),
                state: state.into(),
                commands: vec!["appearances".into(), "appearance.assign-face".into(), "appearance.inherit-face".into(), "appearance.repair-face".into()],
                relationships: vec![part.id.clone(), format!("finish:{}", assignment.finish)],
                data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"definition":part.id,"assignment":assignment,"source":assignment,"appearance_only":true,"resolution":"current-native-exact-unique","fallback":"part-finish","signature_is_identity":false}),
            });
        }
    }
    for assembly in doc.assemblies.values() {
        for instance in assembly.instances.values() {
            out.push(SemanticElement {
                id: format!("{}::finish", instance.id), role: "occurrence-appearance".into(),
                label: format!("{} · Surface Finish", instance.name), value: instance.finish.clone(), state: state.into(),
                commands: vec!["appearances".into(), "appearance.assign-instance".into()], relationships: vec![instance.id.clone(), instance.definition.clone()],
                data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"setter":"appearance-instance","assembly":assembly.id,"instance":instance.id,"finish":instance.finish,"json":instance.finish,"nullable":true,"value_type":"choice","choices":std::iter::once(serde_json::json!({"value":null,"label":"Inherit"})).chain(choices.iter().cloned()).collect::<Vec<_>>(),"editable":true,"edit_command":"appearance.assign-instance","inherit":instance.finish.is_none()}),
            });
        }
    }
    out.push(SemanticElement {
        id: "viewport-lighting".into(), role: "scene-lighting".into(), label: "Studio Lighting".into(), value: None,
        state: state.into(), commands: vec!["lighting".into()], relationships: vec![doc.id.clone()],
        data: serde_json::json!({"contract":"tcad.appearance","version":CONTRACT_VERSION,"source":doc.lighting,"exposure_unit":"stops","rotation_unit":"deg"}),
    });
    let source = serde_json::to_value(&doc.lighting).unwrap_or_default();
    let mut environment = field_element(
        "viewport-lighting::/environment".into(),
        "Environment",
        state,
        "viewport-lighting",
        serde_json::json!(doc.lighting.environment),
        serde_json::json!({"setter":"appearance-lighting","field":"/environment","value_type":"choice","nullable":true,"choices":std::iter::once(serde_json::json!({"value":null,"label":"Built-in studio"})).chain(doc.resources.values().filter(|r|matches!(r.kind.to_ascii_lowercase().as_str(),"hdr"|"exr")).map(|r|serde_json::json!({"value":r.id,"label":r.path}))).collect::<Vec<_>>(),"edit_command":"appearance.edit","projection":"equirectangular-2-to-1","color_space":"linear-rgb","radiance_changes_geometry":false}),
    );
    environment.commands.extend([
        "appearance.environment.import".into(),
        "appearance.environment.clear".into(),
    ]);
    out.push(environment);
    for (field, label, minimum, maximum, unit) in [
        (
            "/environment_rotation_deg",
            "Studio Rotation",
            Some(-1_000_000.),
            Some(1_000_000.),
            Some("deg"),
        ),
        ("/exposure", "Exposure", Some(-8.), Some(8.), Some("stops")),
        (
            "/intensity",
            "Environment Intensity",
            Some(0.),
            Some(10.),
            None,
        ),
        ("/key_intensity", "Key Light", Some(0.), Some(10.), None),
        ("/fill_intensity", "Fill Light", Some(0.), Some(10.), None),
        ("/shadows", "Shadows", None, None, None),
    ] {
        let value = source.pointer(field).cloned().unwrap_or_default();
        out.push(field_element(format!("viewport-lighting::{field}"),label,state,"viewport-lighting",value.clone(),serde_json::json!({"setter":"appearance-lighting","field":field,"value_type":if value.is_boolean(){"boolean"}else{"number"},"minimum":minimum,"maximum":maximum,"unit":unit,"edit_command":"appearance.edit"})));
    }
    out
}
