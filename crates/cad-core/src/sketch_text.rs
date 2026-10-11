//! Application-owned exact local-font outlines and associative sketch text.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ttf_parser::OutlineBuilder;
use unicode_general_category::{GeneralCategory, get_general_category};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_CHARACTERS: usize = 1024;
pub const MAX_SEGMENTS: usize = 32768;
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Line {
        end: [f64; 2],
    },
    Quadratic {
        control: [f64; 2],
        end: [f64; 2],
    },
    Cubic {
        first: [f64; 2],
        second: [f64; 2],
        end: [f64; 2],
    },
}
impl Segment {
    fn end(&self) -> [f64; 2] {
        match self {
            Self::Line { end } | Self::Quadratic { end, .. } | Self::Cubic { end, .. } => *end,
        }
    }
    fn controls(&self, start: [f64; 2]) -> Vec<[f64; 2]> {
        match self {
            Self::Line { end } => vec![start, *end],
            Self::Quadratic { control, end } => vec![start, *control, *end],
            Self::Cubic { first, second, end } => vec![start, *first, *second, *end],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    pub start: [f64; 2],
    pub segments: Vec<Segment>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    pub glyph_index: u16,
    pub advance: f64,
    pub contours: Vec<Contour>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontSnapshot {
    pub resource: Id,
    pub sha256: String,
    pub face_index: u32,
    pub name: String,
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub glyphs: BTreeMap<char, Glyph>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentBinding {
    pub member: Id,
    pub points: Vec<Id>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlyphBinding {
    pub slot: usize,
    pub character: char,
    pub contours: Vec<Vec<SegmentBinding>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub font: FontSnapshot,
    pub text: String,
    pub anchor: Id,
    pub height: Expr,
    pub offset: [Expr; 2],
    pub angle: Expr,
    pub tracking: Expr,
    pub line_spacing: Expr,
    pub alignment: Alignment,
    pub glyphs: Vec<GlyphBinding>,
}
impl Definition {
    pub fn outputs(&self) -> Vec<Id> {
        self.glyphs
            .iter()
            .flat_map(|glyph| &glyph.contours)
            .flatten()
            .map(|segment| segment.member.clone())
            .collect()
    }
    pub fn driven_points(&self) -> Vec<Id> {
        self.glyphs
            .iter()
            .flat_map(|glyph| &glyph.contours)
            .flatten()
            .flat_map(|segment| segment.points.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        vec![
            &self.height,
            &self.offset[0],
            &self.offset[1],
            &self.angle,
            &self.tracking,
            &self.line_spacing,
        ]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Create {
    pub font: FontSnapshot,
    pub text: String,
    pub anchor: Id,
    pub height: Expr,
    pub offset: [Expr; 2],
    pub angle: Expr,
    pub tracking: Expr,
    pub line_spacing: Expr,
    pub alignment: Alignment,
    pub construction: bool,
    #[serde(default)]
    pub apply_construction: bool,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn unsupported_character(character: char) -> bool {
    let category = get_general_category(character);
    // These scripts require bidi/context shaping; an individual cmap glyph is
    // insufficient. Supported text deliberately uses unshaped left-to-right glyphs.
    matches!(
        category,
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
            | GeneralCategory::Format
            | GeneralCategory::Control
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
            | GeneralCategory::Surrogate
            | GeneralCategory::Unassigned
    ) || matches!(character as u32,0x0590..=0x109f|0x1780..=0x18af|0x1900..=0x1cff|0x2d30..=0x2d7f|0xa800..=0xabff|0xfb1d..=0xfdff|0xfe70..=0xfeff|0x10800..=0x11fff|0x1e800..=0x1eeff)
}
pub fn validate_text(text: &str) -> Result<()> {
    if text.is_empty() || text.chars().count() > MAX_CHARACTERS {
        return Err(invalid("Sketch text requires 1–1024 characters"));
    }
    if text
        .chars()
        .any(|character| character != '\n' && unsupported_character(character))
    {
        return Err(CadError::Unsupported("Sketch Text supports unshaped left-to-right glyphs and newlines; combining marks, bidi/contextual scripts, tabs and format controls require shaping support".into()));
    }
    Ok(())
}
#[derive(Default)]
struct Builder {
    contours: Vec<Contour>,
    current: Option<Contour>,
    bad: bool,
    segments: usize,
}
impl Builder {
    fn push(&mut self, segment: Segment) {
        self.segments += 1;
        if self.segments > MAX_SEGMENTS {
            self.bad = true;
            return;
        }
        if let Some(contour) = &mut self.current {
            contour.segments.push(segment);
        } else {
            self.bad = true;
        }
    }
}
impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.current.is_some() {
            self.bad = true;
        }
        self.current = Some(Contour {
            start: [f64::from(x), f64::from(y)],
            segments: vec![],
        });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let end = [f64::from(x), f64::from(y)];
        if self.current.as_ref().is_some_and(|contour| {
            contour
                .segments
                .last()
                .map(Segment::end)
                .unwrap_or(contour.start)
                == end
        }) {
            return;
        }
        self.push(Segment::Line { end });
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push(Segment::Quadratic {
            control: [f64::from(x1), f64::from(y1)],
            end: [f64::from(x), f64::from(y)],
        });
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push(Segment::Cubic {
            first: [f64::from(x1), f64::from(y1)],
            second: [f64::from(x2), f64::from(y2)],
            end: [f64::from(x), f64::from(y)],
        });
    }
    fn close(&mut self) {
        if let Some(mut contour) = self.current.take() {
            if let Some(end) = contour.segments.last().map(Segment::end)
                && end != contour.start
            {
                self.segments += 1;
                contour.segments.push(Segment::Line { end: contour.start });
            }
            if contour.segments.is_empty() || self.segments > MAX_SEGMENTS {
                self.bad = true;
            } else {
                self.contours.push(contour);
            }
        } else {
            self.bad = true;
        }
    }
}
/// Parse the pinned local face once into editable, application-owned polynomial
/// contours. No font engine or tessellation object survives in the source.
pub fn parse_font(
    bytes: &[u8],
    resource: &Resource,
    face_index: u32,
    text: &str,
) -> Result<FontSnapshot> {
    validate_text(text)?;
    if bytes.len() > MAX_FONT_BYTES {
        return Err(invalid("Sketch font resource exceeds 32 MiB"));
    }
    if !matches!(resource.kind.as_str(), "ttf" | "otf" | "ttc" | "otc")
        || format!("{:x}", Sha256::digest(bytes)) != resource.sha256
    {
        return Err(invalid(
            "Pinned font resource has an invalid type or changed bytes",
        ));
    }
    let face = ttf_parser::Face::parse(bytes, face_index)
        .map_err(|_| invalid("Cannot parse the selected TrueType/OpenType font face"))?;
    if face.is_variable() {
        return Err(CadError::Unsupported(
            "Variable fonts require a pinned static font instance".into(),
        ));
    }
    let name = face
        .names()
        .into_iter()
        .filter(|name| name.name_id == ttf_parser::name_id::FULL_NAME)
        .find_map(|name| name.to_string())
        .unwrap_or_else(|| "Local outline font".into());
    let mut glyphs = BTreeMap::new();
    let mut total = 0usize;
    for character in text
        .chars()
        .filter(|character| *character != '\n')
        .collect::<BTreeSet<_>>()
    {
        let glyph_index = face
            .glyph_index(character)
            .filter(|index| index.0 != 0)
            .ok_or_else(|| invalid(&format!("Font has no glyph for U+{:04X}", character as u32)))?;
        let advance = f64::from(
            face.glyph_hor_advance(glyph_index)
                .ok_or_else(|| invalid("Font glyph has no horizontal advance"))?,
        );
        let mut builder = Builder::default();
        let outlined = face.outline_glyph(glyph_index, &mut builder).is_some();
        if builder.bad
            || builder.current.is_some()
            || (!outlined && (!character.is_whitespace() || !builder.contours.is_empty()))
            || (outlined && builder.contours.is_empty())
        {
            return Err(CadError::Unsupported(format!(
                "Glyph U+{:04X} has no complete supported polynomial outline",
                character as u32
            )));
        }
        total = total
            .checked_add(builder.segments)
            .ok_or_else(|| invalid("Font outline count overflow"))?;
        if total > MAX_SEGMENTS {
            return Err(invalid("Selected text has too many exact outline segments"));
        }
        glyphs.insert(
            character,
            Glyph {
                glyph_index: glyph_index.0,
                advance,
                contours: builder.contours,
            },
        );
    }
    let font = FontSnapshot {
        resource: resource.id.clone(),
        sha256: resource.sha256.clone(),
        face_index,
        name,
        units_per_em: face.units_per_em(),
        ascender: face.ascender(),
        descender: face.descender(),
        line_gap: face.line_gap(),
        glyphs,
    };
    validate_font(&font)?;
    Ok(font)
}
fn validate_font(font: &FontSnapshot) -> Result<()> {
    if font.resource.is_empty()
        || font.sha256.len() != 64
        || !font.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        || font.units_per_em == 0
        || font.glyphs.len() > MAX_CHARACTERS
    {
        return Err(invalid("Invalid pinned font snapshot metadata"));
    }
    let mut total = 0usize;
    for (character, glyph) in &font.glyphs {
        if unsupported_character(*character)
            || !glyph.advance.is_finite()
            || glyph.advance < 0.
            || glyph.glyph_index == 0
        {
            return Err(invalid("Invalid font glyph metrics or character"));
        }
        for contour in &glyph.contours {
            if contour.segments.is_empty()
                || contour.segments.last().map(Segment::end) != Some(contour.start)
                || contour.start.iter().any(|v| !v.is_finite())
            {
                return Err(invalid("Font outlines require finite closed contours"));
            }
            let mut start = contour.start;
            for segment in &contour.segments {
                if segment
                    .controls(start)
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite())
                {
                    return Err(invalid("Font outline controls must be finite"));
                }
                start = segment.end();
                total += 1;
            }
            if total > MAX_SEGMENTS {
                return Err(invalid("Font snapshot exceeds the outline segment limit"));
            }
        }
    }
    Ok(())
}
pub fn validate_font_resource(doc: &Document, font: &FontSnapshot) -> Result<()> {
    validate_font(font)?;
    let resource = doc
        .resources
        .get(&font.resource)
        .ok_or_else(|| invalid("Sketch Text pinned font resource is missing"))?;
    if resource.sha256 != font.sha256
        || !matches!(resource.kind.as_str(), "ttf" | "otf" | "ttc" | "otc")
    {
        return Err(invalid(
            "Sketch Text font resource does not match its outline snapshot",
        ));
    }
    Ok(())
}
pub fn operation_for<'a>(sketch: &'a Sketch, member: &str) -> Option<(&'a Id, &'a Definition)> {
    sketch
        .texts
        .iter()
        .find(|(_, definition)| definition.outputs().iter().any(|id| id == member))
}
fn curve(points: &[Id]) -> SketchCurve {
    if points.len() == 2 {
        SketchCurve::Line {
            start: points[0].clone(),
            end: points[1].clone(),
        }
    } else {
        SketchCurve::Bezier {
            points: points.to_vec(),
        }
    }
}
pub fn validate(sketch: &Sketch, definition: &Definition) -> Result<()> {
    validate_text(&definition.text)?;
    validate_font(&definition.font)?;
    if !sketch.points.contains_key(&definition.anchor) {
        return Err(invalid("Sketch Text insertion anchor is missing"));
    }
    let expected = definition
        .text
        .chars()
        .enumerate()
        .filter(|(_, character)| *character != '\n')
        .collect::<Vec<_>>();
    if expected.len() != definition.glyphs.len() {
        return Err(invalid("Sketch Text layout has stale character bindings"));
    }
    let mut members = BTreeSet::new();
    let mut controls = BTreeSet::new();
    let mut total = 0;
    for ((slot, character), binding) in expected.into_iter().zip(&definition.glyphs) {
        let glyph = definition
            .font
            .glyphs
            .get(&character)
            .ok_or_else(|| invalid("Font snapshot is missing a text glyph"))?;
        if binding.slot != slot
            || binding.character != character
            || binding.contours.len() != glyph.contours.len()
        {
            return Err(invalid(
                "Sketch Text has inconsistent glyph contour bindings",
            ));
        }
        for (source, bound) in glyph.contours.iter().zip(&binding.contours) {
            if source.segments.len() != bound.len() {
                return Err(invalid("Sketch Text segment binding count is inconsistent"));
            }
            for (segment, binding) in source.segments.iter().zip(bound) {
                total += 1;
                let count = segment.controls(source.start).len();
                if binding.points.len() != count
                    || binding
                        .points
                        .iter()
                        .any(|id| id == &definition.anchor || !sketch.points.contains_key(id))
                    || !members.insert(&binding.member)
                    || sketch
                        .members
                        .get(&binding.member)
                        .is_none_or(|member| member.curve != curve(&binding.points))
                {
                    return Err(invalid(
                        "Sketch Text has missing, duplicate or inconsistent output topology",
                    ));
                }
                if binding.points[..binding.points.len() - 1]
                    .iter()
                    .any(|point| !controls.insert(point.clone()))
                {
                    return Err(invalid(
                        "Text contours reuse an unrelated output control identity",
                    ));
                }
            }
            for pair in bound.windows(2) {
                if pair[0].points.last() != pair[1].points.first() {
                    return Err(invalid(
                        "Text contour endpoints do not share their exact source identities",
                    ));
                }
            }
            if bound.first().and_then(|segment| segment.points.first())
                != bound.last().and_then(|segment| segment.points.last())
            {
                return Err(invalid("Sketch Text output contour is open"));
            }
        }
    }
    if total == 0 || total > MAX_SEGMENTS {
        return Err(invalid(
            "Sketch Text requires nonempty outlines within 32768 segments",
        ));
    }
    Ok(())
}
pub fn derive(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    definition: &Definition,
) -> Result<()> {
    validate(sketch, definition)?;
    validate_font_resource(doc, &definition.font)?;
    let height = expression::value(doc, owner, &definition.height, Dimension::Length)?;
    let scale = height / f64::from(definition.font.units_per_em);
    let tracking = expression::value(doc, owner, &definition.tracking, Dimension::Length)?;
    let line_spacing = expression::value(doc, owner, &definition.line_spacing, Dimension::Scalar)?;
    let angle = expression::value(doc, owner, &definition.angle, Dimension::Angle)?;
    let anchor = sketch.points[&definition.anchor];
    let offset = [
        expression::value(doc, owner, &definition.offset[0], Dimension::Length)?,
        expression::value(doc, owner, &definition.offset[1], Dimension::Length)?,
    ];
    if height <= 1e-7 || !scale.is_finite() || scale <= 0. || line_spacing <= 0. {
        return Err(invalid(
            "Text em-height and line spacing must be positive and finite",
        ));
    }
    let (sin, cos) = angle.sin_cos();
    let mut line = 0usize;
    let mut x = 0.;
    let mut glyph_index = 0usize;
    let line_widths = definition
        .text
        .split('\n')
        .map(|text| {
            let mut width = 0.;
            let count = text.chars().count();
            for (index, character) in text.chars().enumerate() {
                let advance = definition.font.glyphs[&character].advance * scale;
                if index + 1 < count && advance + tracking <= 0. {
                    return Err(invalid(
                        "Text spacing reverses or collapses a glyph advance",
                    ));
                }
                width += advance + if index + 1 < count { tracking } else { 0. };
            }
            if width.is_finite() {
                Ok(width)
            } else {
                Err(invalid("Text line width overflow"))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    for character in definition.text.chars() {
        if character == '\n' {
            line += 1;
            x = 0.;
            continue;
        }
        let glyph = &definition.font.glyphs[&character];
        let binding = &definition.glyphs[glyph_index];
        glyph_index += 1;
        let shift = match definition.alignment {
            Alignment::Left => 0.,
            Alignment::Center => -line_widths[line] / 2.,
            Alignment::Right => -line_widths[line],
        };
        for (contour, bound) in glyph.contours.iter().zip(&binding.contours) {
            let mut start = contour.start;
            for (segment, binding) in contour.segments.iter().zip(bound) {
                for (raw, key) in segment.controls(start).iter().zip(&binding.points) {
                    let local = [
                        raw[0] * scale + x + shift,
                        raw[1] * scale - (line as f64) * height * line_spacing,
                    ];
                    let point = [
                        anchor[0] + offset[0] + cos * local[0] - sin * local[1],
                        anchor[1] + offset[1] + sin * local[0] + cos * local[1],
                    ];
                    if point.iter().any(|v| !v.is_finite()) {
                        return Err(invalid("Text outline placement coordinate overflow"));
                    }
                    sketch.points.insert(key.clone(), point);
                }
                start = segment.end();
            }
        }
        x += glyph.advance * scale + tracking;
        if !x.is_finite() {
            return Err(invalid("Text glyph advance overflow"));
        }
    }
    Ok(())
}
/// Regenerate bindings on a candidate only. Compatible glyph/contour/segment
/// slots retain their exact IDs; a removed referenced output requires repair.
pub fn set(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    request: &Create,
) -> Result<()> {
    validate_text(&request.text)?;
    validate_font_resource(doc, &request.font)?;
    if !sketch.points.contains_key(&request.anchor) {
        return Err(invalid("Select or create a text insertion point"));
    }
    let mut candidate = sketch.clone();
    let old = candidate.texts.remove(key);
    let mut glyphs = vec![];
    let mut total = 0usize;
    for (slot, character) in request
        .text
        .chars()
        .enumerate()
        .filter(|(_, character)| *character != '\n')
    {
        let glyph = request.font.glyphs.get(&character).ok_or_else(|| {
            invalid("Import or refresh the font snapshot for the new text characters")
        })?;
        let previous = old.as_ref().and_then(|definition| {
            definition
                .glyphs
                .iter()
                .find(|binding| binding.slot == slot)
        });
        let mut contours = vec![];
        for (contour_index, contour) in glyph.contours.iter().enumerate() {
            total += contour.segments.len();
            if total > MAX_SEGMENTS {
                return Err(invalid("Text exceeds 32768 exact outline segments"));
            }
            let previous = previous.and_then(|glyph| glyph.contours.get(contour_index));
            let compatible = previous.is_some_and(|bound| {
                bound.len() == contour.segments.len()
                    && bound
                        .iter()
                        .zip(&contour.segments)
                        .all(|(binding, segment)| {
                            binding.points.len() == segment.controls(contour.start).len()
                        })
            });
            let bindings = if compatible {
                previous.unwrap().clone()
            } else {
                let first = id();
                let mut start = first.clone();
                let mut bound = vec![];
                for (index, segment) in contour.segments.iter().enumerate() {
                    let count = segment.controls(contour.start).len();
                    let mut points = vec![start];
                    points.extend((0..count - 2).map(|_| id()));
                    let end = if index + 1 == contour.segments.len() {
                        first.clone()
                    } else {
                        id()
                    };
                    points.push(end.clone());
                    start = end;
                    bound.push(SegmentBinding {
                        member: id(),
                        points,
                    });
                }
                bound
            };
            for binding in &bindings {
                for point in &binding.points {
                    candidate.points.entry(point.clone()).or_insert([0.; 2]);
                }
                let construction = if request.apply_construction {
                    request.construction
                } else {
                    candidate
                        .members
                        .get(&binding.member)
                        .map(|member| member.construction)
                        .unwrap_or(request.construction)
                };
                candidate.members.insert(
                    binding.member.clone(),
                    SketchMember {
                        construction,
                        curve: curve(&binding.points),
                    },
                );
            }
            contours.push(bindings);
        }
        glyphs.push(GlyphBinding {
            slot,
            character,
            contours,
        });
    }
    let definition = Definition {
        font: request.font.clone(),
        text: request.text.clone(),
        anchor: request.anchor.clone(),
        height: request.height.clone(),
        offset: request.offset.clone(),
        angle: request.angle.clone(),
        tracking: request.tracking.clone(),
        line_spacing: request.line_spacing.clone(),
        alignment: request.alignment,
        glyphs,
    };
    if let Some(old) = old {
        let members = definition.outputs().into_iter().collect::<BTreeSet<_>>();
        let points = definition
            .driven_points()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let retired_members = old
            .outputs()
            .into_iter()
            .filter(|member| !members.contains(member))
            .collect::<BTreeSet<_>>();
        let retired_points = old
            .driven_points()
            .into_iter()
            .filter(|point| !points.contains(point))
            .collect::<BTreeSet<_>>();
        if candidate.constraints.iter().any(|constraint| {
            constraint
                .entities
                .iter()
                .any(|entity| retired_members.contains(entity) || retired_points.contains(entity))
        }) || candidate.members.iter().any(|(member, record)| {
            !retired_members.contains(member)
                && record
                    .curve
                    .points()
                    .iter()
                    .any(|point| retired_points.contains(*point))
        }) || crate::sketch_edit::modifier_ids(&candidate)
            .iter()
            .any(|modifier| {
                crate::sketch_edit::modifier_sources(&candidate, modifier)
                    .iter()
                    .any(|member| retired_members.contains(member))
                    || crate::sketch_edit::modifier_reference_points(&candidate, modifier)
                        .iter()
                        .any(|point| retired_points.contains(point))
            })
        {
            return Err(invalid(
                "Text edit retires referenced outline members or controls; detach or repair those dependencies first",
            ));
        }
        for member in retired_members {
            candidate.members.remove(&member);
        }
        for point in retired_points {
            candidate.points.remove(&point);
        }
    }
    candidate.texts.insert(key.into(), definition.clone());
    derive(doc, owner, &mut candidate, &definition)?;
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok(())
}
