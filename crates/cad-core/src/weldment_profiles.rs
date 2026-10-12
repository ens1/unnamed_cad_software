//! Portable nominal structural sections, independent of the geometry kernel and renderer.
//!
//! Contours are authored in millimetres. The first contour is the material exterior;
//! subsequent contours are holes. Import checks are bounded dimensional/topological
//! sanity checks, not a substitute for current native section validation at Apply.
use crate::{CadError, Id, Result, Sketch, SketchConstraint, SketchCurve, SketchMember};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
const EMBEDDED: &str = include_str!("../../../assets/weldment-profiles/nominal-sections.json");
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROFILES: usize = 2000;
const MAX_CONTOUR_POINTS: usize = 256;
const MAX_CONTOURS: usize = 17;
const MAX_PROFILE_POINTS: usize = 1024;
const MAX_CATALOG_POINTS: usize = 32_768;
const MAX_VALIDATION_PAIRS: usize = 4_000_000;
const MIN_SEPARATION_MM: f64 = 1e-6;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    SquareTube,
    RectangularTube,
    RoundTube,
    Angle,
    Channel,
    IBeam,
    FlatBar,
    SolidRound,
    SolidSquare,
    Custom,
}
impl Family {
    pub const ALL: [Self; 10] = [
        Self::SquareTube,
        Self::RectangularTube,
        Self::RoundTube,
        Self::Angle,
        Self::Channel,
        Self::IBeam,
        Self::FlatBar,
        Self::SolidRound,
        Self::SolidSquare,
        Self::Custom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::SquareTube => "Square tube",
            Self::RectangularTube => "Rectangular tube",
            Self::RoundTube => "Round tube",
            Self::Angle => "Angle",
            Self::Channel => "Channel",
            Self::IBeam => "I beam",
            Self::FlatBar => "Flat bar",
            Self::SolidRound => "Solid round",
            Self::SolidSquare => "Solid square",
            Self::Custom => "Custom profile",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NominalUnits {
    Metric,
    Imperial,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub id: Id,
    pub label: String,
    pub units: NominalUnits,
    pub notes: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Contour {
    Polygon { points: Vec<[f64; 2]> },
    Circle { center: [f64; 2], radius_mm: f64 },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocatingPoint {
    pub id: Id,
    pub name: String,
    pub point: [f64; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: Id,
    pub revision: String,
    pub collection: Id,
    pub family: Family,
    pub label: String,
    pub size: String,
    pub notes: String,
    pub contours: Vec<Contour>,
    pub locators: Vec<LocatingPoint>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub id: Id,
    pub revision: String,
    pub name: String,
    pub collections: Vec<Collection>,
    pub profiles: Vec<Entry>,
}
/// Saved complete source snapshot. Catalog reloads never mutate an existing member.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub version: u32,
    pub catalog_id: Id,
    pub catalog_revision: String,
    pub catalog_name: String,
    pub profile: Entry,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Locator {
    pub id: Id,
    pub name: String,
    pub point: [f64; 2],
    /// Fixed sketch point used by the member's existing locating-point contract.
    pub sketch_point: Id,
}
#[derive(Clone, Debug)]
pub struct Instantiation {
    pub sketch: Sketch,
    pub locators: Vec<Locator>,
    pub origin: Origin,
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Structural profile library: {}", message.into()))
}
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn identity(value: &str) -> bool {
    text(value, 256) && !matches!(value, "." | "..") && !value.contains(['/', '\\', ':'])
}
fn notes(value: &str) -> bool {
    value.len() <= 8192
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn point(value: &[f64; 2]) -> bool {
    value.iter().all(|v| v.is_finite() && v.abs() <= 100_000.)
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn point_segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t =
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1])).clamp(0., 1.);
    distance(p, [a[0] + t * d[0], a[1] + t * d[1]])
}
fn segment_distance(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let opposite = |x: f64, y: f64| (x <= 0. && y >= 0.) || (x >= 0. && y <= 0.);
    if opposite(orient(a, b, c), orient(a, b, d)) && opposite(orient(c, d, a), orient(c, d, b)) {
        // Collinear separated segments need their interval separation, not just signs.
        let overlaps =
            (0..2).all(|i| a[i].min(b[i]) <= c[i].max(d[i]) && c[i].min(d[i]) <= a[i].max(b[i]));
        if overlaps {
            return 0.;
        }
    }
    point_segment_distance(a, c, d)
        .min(point_segment_distance(b, c, d))
        .min(point_segment_distance(c, a, b))
        .min(point_segment_distance(d, a, b))
}
fn polygon_edges(points: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(points.len())
}
fn contour_validate(contour: &Contour) -> Result<()> {
    match contour {
        Contour::Circle { center, radius_mm } => {
            if !point(center)
                || !radius_mm.is_finite()
                || !(1e-5..=100_000.).contains(radius_mm)
                || center.iter().any(|v| v.abs() + radius_mm > 100_000.)
            {
                return Err(invalid(
                    "A circle needs a finite, positive, bounded radius and center",
                ));
            }
        }
        Contour::Polygon { points } => {
            if !(3..=MAX_CONTOUR_POINTS).contains(&points.len()) || points.iter().any(|p| !point(p))
            {
                return Err(invalid("A polygon needs 3–256 finite, bounded vertices"));
            }
            let edges = polygon_edges(points).collect::<Vec<_>>();
            if edges.iter().any(|(a, b)| distance(*a, *b) < 1e-5) {
                return Err(invalid("A polygon contains a collapsed or undersized edge"));
            }
            let origin = points[0];
            let twice_area: f64 = edges.iter().map(|(a, b)| orient(origin, *a, *b)).sum();
            if twice_area.abs() < 1e-8 {
                return Err(invalid("A polygon has no usable material area"));
            }
            for (i, (a, b)) in edges.iter().enumerate() {
                let c = points[(i + 2) % points.len()];
                // Collinear corners are ambiguous authored vertices; remove them on import.
                if orient(*a, *b, c).abs() / distance(*a, c).max(1e-5) <= MIN_SEPARATION_MM {
                    return Err(invalid(
                        "A polygon contains a collinear or uncertain corner",
                    ));
                }
                for (j, (c, d)) in edges.iter().enumerate().skip(i + 1) {
                    if j == i + 1 || (i == 0 && j == edges.len() - 1) {
                        continue;
                    }
                    if segment_distance(*a, *b, *c, *d) <= MIN_SEPARATION_MM {
                        return Err(invalid(
                            "Polygon edges intersect or are closer than 0.000001 mm",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
fn contains(contour: &Contour, p: [f64; 2]) -> bool {
    match contour {
        Contour::Circle { center, radius_mm } => distance(*center, p) < *radius_mm,
        Contour::Polygon { points } => {
            let mut inside = false;
            for (a, b) in polygon_edges(points) {
                if (a[1] > p[1]) != (b[1] > p[1])
                    && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
                {
                    inside = !inside;
                }
            }
            inside
        }
    }
}
fn representative(contour: &Contour) -> [f64; 2] {
    match contour {
        Contour::Circle { center, radius_mm } => [center[0] + radius_mm, center[1]],
        Contour::Polygon { points } => points[0],
    }
}
fn separated(a: &Contour, b: &Contour) -> bool {
    match (a, b) {
        (
            Contour::Circle {
                center: a,
                radius_mm: ar,
            },
            Contour::Circle {
                center: b,
                radius_mm: br,
            },
        ) => {
            let d = distance(*a, *b);
            d > ar + br + MIN_SEPARATION_MM || d + ar.min(*br) + MIN_SEPARATION_MM < ar.max(*br)
        }
        (Contour::Polygon { points: a }, Contour::Polygon { points: b }) => {
            polygon_edges(a).all(|(a, b0)| {
                polygon_edges(b).all(|(c, d)| segment_distance(a, b0, c, d) > MIN_SEPARATION_MM)
            })
        }
        (Contour::Circle { center, radius_mm }, Contour::Polygon { points })
        | (Contour::Polygon { points }, Contour::Circle { center, radius_mm }) => {
            polygon_edges(points).all(|(a, b)| {
                let minimum = point_segment_distance(*center, a, b);
                let maximum = distance(*center, a).max(distance(*center, b));
                minimum > radius_mm + MIN_SEPARATION_MM || maximum + MIN_SEPARATION_MM < *radius_mm
            })
        }
    }
}
impl Entry {
    pub fn display_name(&self) -> String {
        if self.label.contains(&self.size) {
            self.label.clone()
        } else {
            format!("{} · {}", self.label, self.size)
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !identity(&self.id)
            || !identity(&self.collection)
            || !text(&self.revision, 128)
            || !text(&self.label, 512)
            || !text(&self.size, 128)
            || !notes(&self.notes)
            || self.contours.is_empty()
            || self.contours.len() > MAX_CONTOURS
            || self.locators.is_empty()
            || self.locators.len() > 64
        {
            return Err(invalid(
                "A profile needs bounded identities, contours and named locating points",
            ));
        }
        let points = point_count(self);
        if points > MAX_PROFILE_POINTS {
            return Err(invalid("A profile exceeds the 1024 contour-control limit"));
        }
        let polygons = self
            .contours
            .iter()
            .filter(|contour| matches!(contour, Contour::Polygon { .. }))
            .count();
        let circles = self.contours.len() - polygons;
        let family_geometry = match self.family {
            Family::RoundTube => circles == 2 && polygons == 0,
            Family::SolidRound => circles == 1 && polygons == 0,
            Family::SquareTube | Family::RectangularTube => polygons == 2 && circles == 0,
            Family::Angle
            | Family::Channel
            | Family::IBeam
            | Family::FlatBar
            | Family::SolidSquare => polygons == 1 && circles == 0,
            Family::Custom => true,
        };
        if !family_geometry {
            return Err(invalid(
                "The profile family does not match its contour types; use Custom for another section",
            ));
        }
        for contour in &self.contours {
            contour_validate(contour)?;
        }
        validate_family_dimensions(self)?;
        let outer = &self.contours[0];
        for (i, hole) in self.contours.iter().enumerate().skip(1) {
            if !separated(outer, hole) || !contains(outer, representative(hole)) {
                return Err(invalid(
                    "Every hole must lie strictly inside the exterior without touching it",
                ));
            }
            for other in self.contours.iter().skip(1).take(i - 1) {
                if !separated(hole, other)
                    || contains(hole, representative(other))
                    || contains(other, representative(hole))
                {
                    return Err(invalid(
                        "Profile holes intersect, touch or contain another hole",
                    ));
                }
            }
        }
        let mut ids = BTreeSet::new();
        for locator in &self.locators {
            if !identity(&locator.id)
                || !ids.insert(&locator.id)
                || !text(&locator.name, 256)
                || !point(&locator.point)
            {
                return Err(invalid(
                    "Locating points need unique identities, names and finite millimetre coordinates",
                ));
            }
        }
        Ok(())
    }
}
fn rectangle_bounds(contour: &Contour) -> Option<[[f64; 2]; 2]> {
    let Contour::Polygon { points } = contour else {
        return None;
    };
    if points.len() != 4 {
        return None;
    }
    let min = [
        points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
    ];
    let max = [
        points
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max),
        points
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    if points
        .iter()
        .all(|p| (p[0] == min[0] || p[0] == max[0]) && (p[1] == min[1] || p[1] == max[1]))
        && polygon_edges(points).all(|(a, b)| (a[0] == b[0]) != (a[1] == b[1]))
    {
        Some([min, max])
    } else {
        None
    }
}
fn validate_family_dimensions(entry: &Entry) -> Result<()> {
    let same = |a: f64, b: f64| (a - b).abs() <= MIN_SEPARATION_MM;
    let polygon_vertices =
        |count| matches!(&entry.contours[0], Contour::Polygon { points } if points.len() == count);
    let okay = match entry.family {
        Family::SquareTube | Family::RectangularTube => {
            match (
                rectangle_bounds(&entry.contours[0]),
                rectangle_bounds(&entry.contours[1]),
            ) {
                (Some([a, b]), Some([c, d])) => {
                    let gaps = [c[0] - a[0], c[1] - a[1], b[0] - d[0], b[1] - d[1]];
                    gaps.iter()
                        .all(|gap| *gap > MIN_SEPARATION_MM && same(*gap, gaps[0]))
                        && (entry.family != Family::SquareTube || same(b[0] - a[0], b[1] - a[1]))
                }
                _ => false,
            }
        }
        Family::RoundTube => {
            matches!((&entry.contours[0],&entry.contours[1]), (Contour::Circle { center:a,radius_mm:ar },Contour::Circle { center:b,radius_mm:br }) if a == b && ar > br)
        }
        Family::FlatBar | Family::SolidSquare => {
            rectangle_bounds(&entry.contours[0]).is_some_and(|[a, b]| {
                entry.family != Family::SolidSquare || same(b[0] - a[0], b[1] - a[1])
            })
        }
        Family::Angle => polygon_vertices(6),
        Family::Channel => polygon_vertices(8),
        Family::IBeam => polygon_vertices(12),
        Family::SolidRound | Family::Custom => true,
    };
    if okay {
        Ok(())
    } else {
        Err(invalid(
            "The named profile family is inconsistent with its nominal shape; use Custom for another section",
        ))
    }
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !identity(&self.id)
            || !text(&self.revision, 128)
            || !text(&self.name, 1024)
            || self.collections.is_empty()
            || self.collections.len() > 64
            || self.profiles.is_empty()
            || self.profiles.len() > MAX_PROFILES
        {
            return Err(invalid(
                "The catalog version, identities or collection/profile limits are invalid",
            ));
        }
        let mut collections = BTreeSet::new();
        for collection in &self.collections {
            if !identity(&collection.id)
                || !collections.insert(&collection.id)
                || !text(&collection.label, 512)
                || !notes(&collection.notes)
            {
                return Err(invalid("A profile collection is invalid or duplicated"));
            }
        }
        let mut entries = BTreeSet::new();
        let mut points = 0usize;
        let mut pairs = 0usize;
        for entry in &self.profiles {
            let count = point_count(entry);
            points = points.saturating_add(count);
            pairs = pairs.saturating_add(count.saturating_mul(count));
            if points > MAX_CATALOG_POINTS || pairs > MAX_VALIDATION_PAIRS {
                return Err(invalid(
                    "The catalog exceeds its bounded contour-control or geometric validation budget",
                ));
            }
            entry.validate()?;
            if !entries.insert(&entry.id) || !collections.contains(&entry.collection) {
                return Err(invalid(
                    "A profile is duplicated or refers to a missing collection",
                ));
            }
        }
        Ok(())
    }
    pub fn profile(&self, id: &str) -> Option<&Entry> {
        self.profiles.iter().find(|entry| entry.id == id)
    }
    pub fn list(&self, collection: Option<&str>, family: Option<Family>) -> Vec<&Entry> {
        self.profiles
            .iter()
            .filter(|p| {
                collection.is_none_or(|c| p.collection == c) && family.is_none_or(|f| p.family == f)
            })
            .collect()
    }
    pub fn instantiate(&self, profile_id: &str, sketch_id: &str) -> Result<Instantiation> {
        self.validate()?;
        let profile = self
            .profile(profile_id)
            .ok_or_else(|| invalid("The selected profile no longer exists in this catalog"))?;
        Origin {
            version: CONTRACT_VERSION,
            catalog_id: self.id.clone(),
            catalog_revision: self.revision.clone(),
            catalog_name: self.name.clone(),
            profile: profile.clone(),
        }
        .instantiate(sketch_id)
    }
}
fn point_count(entry: &Entry) -> usize {
    entry
        .contours
        .iter()
        .map(|contour| match contour {
            Contour::Polygon { points } => points.len(),
            Contour::Circle { .. } => 1,
        })
        .fold(0, usize::saturating_add)
}
impl Origin {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !identity(&self.catalog_id)
            || !text(&self.catalog_revision, 128)
            || !text(&self.catalog_name, 1024)
        {
            return Err(invalid(
                "The saved profile snapshot version or identity is invalid",
            ));
        }
        self.profile.validate()
    }
    pub fn display_name(&self) -> String {
        self.profile.display_name()
    }
    /// Compare authored source modulo local identities (library insertion remaps
    /// those identities). A renamed sketch is still the same pinned section;
    /// edited geometry, construction flags or expression sources are not.
    pub fn matches_sketch(&self, sketch: &Sketch) -> Result<bool> {
        let generated = self.instantiate(&sketch.id)?.sketch;
        Ok(source_signature(sketch)? == source_signature(&generated)?)
    }
    /// Deterministic digest of the complete pinned source, never an external file path.
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn instantiate(&self, sketch_id: &str) -> Result<Instantiation> {
        self.validate()?;
        if !identity(sketch_id) {
            return Err(invalid(
                "A generated section needs a portable stable sketch identity",
            ));
        }
        let mut sketch = Sketch {
            id: sketch_id.into(),
            name: self.display_name(),
            points: BTreeMap::new(),
            segments: vec![],
            constraints: vec![],
            profile_order: vec![],
            circles: BTreeMap::new(),
            members: BTreeMap::new(),
            conversions: BTreeMap::new(),
            offsets: BTreeMap::new(),
            fillets: BTreeMap::new(),
            chamfers: BTreeMap::new(),
            transforms: BTreeMap::new(),
            splines: BTreeMap::new(),
            conics: BTreeMap::new(),
            texts: BTreeMap::new(),
            equations: BTreeMap::new(),
            blocks: BTreeMap::new(),
            block_instances: BTreeMap::new(),
            dimension_groups: BTreeMap::new(),
        };
        for (index, contour) in self.profile.contours.iter().enumerate() {
            match contour {
                Contour::Circle { center, radius_mm } => {
                    let center =
                        fixed_point(&mut sketch, &format!("contour-{index}-center"), *center);
                    sketch.members.insert(
                        stable(sketch_id, &format!("contour-{index}-circle")),
                        SketchMember {
                            construction: false,
                            curve: SketchCurve::Circle {
                                center,
                                radius: crate::Expr(format!("{radius_mm} mm")),
                            },
                        },
                    );
                }
                Contour::Polygon { points } => {
                    let ids = points
                        .iter()
                        .enumerate()
                        .map(|(vertex, p)| {
                            fixed_point(
                                &mut sketch,
                                &format!("contour-{index}-vertex-{vertex}"),
                                *p,
                            )
                        })
                        .collect::<Vec<_>>();
                    for vertex in 0..ids.len() {
                        sketch.members.insert(
                            stable(sketch_id, &format!("contour-{index}-edge-{vertex}")),
                            SketchMember {
                                construction: false,
                                curve: SketchCurve::Line {
                                    start: ids[vertex].clone(),
                                    end: ids[(vertex + 1) % ids.len()].clone(),
                                },
                            },
                        );
                    }
                }
            }
        }
        // Locator controls are construction points: they participate in fixed point
        // constraints but do not belong to any material contour. They cannot add a
        // stray open chain or silently join independent exterior/hole vertices.
        let locators = self
            .profile
            .locators
            .iter()
            .map(|locator| Locator {
                id: locator.id.clone(),
                name: locator.name.clone(),
                point: locator.point,
                sketch_point: fixed_point(
                    &mut sketch,
                    &format!("locator-{}", locator.id),
                    locator.point,
                ),
            })
            .collect();
        Ok(Instantiation {
            sketch,
            locators,
            origin: self.clone(),
        })
    }
}
fn source_signature(sketch: &Sketch) -> Result<Option<Vec<String>>> {
    if !sketch.segments.is_empty()
        || !sketch.profile_order.is_empty()
        || !sketch.circles.is_empty()
        || !sketch.conversions.is_empty()
        || !sketch.offsets.is_empty()
        || !sketch.fillets.is_empty()
        || !sketch.chamfers.is_empty()
        || !sketch.transforms.is_empty()
        || !sketch.splines.is_empty()
        || !sketch.conics.is_empty()
        || !sketch.texts.is_empty()
        || !sketch.equations.is_empty()
        || !sketch.blocks.is_empty()
        || !sketch.block_instances.is_empty()
        || !sketch.dimension_groups.is_empty()
        || sketch.points.len() > MAX_CONTOURS * MAX_CONTOUR_POINTS + 64
        || sketch.members.len() > MAX_CONTOURS * MAX_CONTOUR_POINTS
        || sketch.constraints.len() > 2 * (MAX_CONTOURS * MAX_CONTOUR_POINTS + 64)
    {
        return Ok(None);
    }
    let mut signature = vec![];
    let coordinates = |id: &str| {
        sketch
            .points
            .get(id)
            .ok_or_else(|| invalid("A pinned section has a missing source point"))
    };
    for p in sketch.points.values() {
        if !point(p) {
            return Ok(None);
        }
        signature.push(serde_json::to_string(&serde_json::json!({"point":p}))?);
    }
    for member in sketch.members.values() {
        if member.construction {
            return Ok(None);
        }
        let value = match &member.curve {
            SketchCurve::Line { start, end } => {
                let mut ends = [
                    serde_json::to_string(coordinates(start)?)?,
                    serde_json::to_string(coordinates(end)?)?,
                ];
                ends.sort();
                serde_json::json!({"line":ends})
            }
            SketchCurve::Circle { center, radius } => {
                serde_json::json!({"circle":coordinates(center)?,"radius":radius})
            }
            _ => return Ok(None),
        };
        signature.push(serde_json::to_string(&value)?);
    }
    let mut fixed_axes = BTreeSet::new();
    for constraint in &sketch.constraints {
        if !matches!(constraint.kind.as_str(), "fix_x" | "fix_y")
            || constraint.entities.len() != 1
            || constraint.driven
            || constraint.contact.is_some()
            || constraint.distance_side.is_some()
        {
            return Ok(None);
        }
        let axis = if constraint.kind == "fix_x" { 0 } else { 1 };
        if !fixed_axes.insert((constraint.entities[0].as_str(), axis))
            || constraint.value.0 != format!("{} mm", coordinates(&constraint.entities[0])?[axis])
        {
            return Ok(None);
        }
        signature.push(serde_json::to_string(&serde_json::json!({"constraint":constraint.kind,"point":coordinates(&constraint.entities[0])?,"value":constraint.value}))?);
    }
    if fixed_axes.len() != sketch.points.len() * 2 {
        return Ok(None);
    }
    signature.sort();
    Ok(Some(signature))
}
fn stable(owner: &str, name: &str) -> Id {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("tcad.weldment-profile.v1:{owner}:{name}"))
    );
    format!(
        "{}-{}-{}-{}-{}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}
fn fixed_point(sketch: &mut Sketch, name: &str, value: [f64; 2]) -> Id {
    let id = stable(&sketch.id, name);
    sketch.points.insert(id.clone(), value);
    for (axis, value) in ["x", "y"].into_iter().zip(value) {
        sketch.constraints.push(SketchConstraint {
            id: stable(&id, axis),
            kind: format!("fix_{axis}"),
            entities: vec![id.clone()],
            value: crate::Expr(format!("{value} mm")),
            driven: false,
            contact: None,
            distance_side: None,
        });
    }
    id
}
pub fn parse_catalog(json: &str) -> Result<Catalog> {
    if json.len() > MAX_JSON_BYTES {
        return Err(invalid("Catalog JSON exceeds 4 MiB"));
    }
    let catalog: Catalog = serde_json::from_str(json)?;
    catalog.validate()?;
    Ok(catalog)
}
pub fn catalog_json(catalog: &Catalog) -> Result<String> {
    catalog.validate()?;
    let json = serde_json::to_string_pretty(catalog)?;
    if json.len() > MAX_JSON_BYTES {
        return Err(invalid("Catalog JSON exceeds 4 MiB"));
    }
    Ok(json)
}
pub fn builtin() -> Result<Catalog> {
    static CATALOG: std::sync::OnceLock<std::result::Result<Catalog, String>> =
        std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| parse_catalog(EMBEDDED).map_err(|error| error.to_string()))
        .clone()
        .map_err(CadError::Invalid)
}
