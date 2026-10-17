//! Versioned engineering annotations and shared f64 manufacturing vectors.
use crate::{
    CadError, Dimension, Document, Drawing, DrawingDimension, Expr, Id, NodeRef, Result,
    TopologyRef,
    protocol::{EdgeData, Evaluation},
};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod symbols;
pub use symbols::*;

pub const CONTRACT_VERSION: u32 = 12;
pub const FORMAT_VERSION: u32 = 4;
const TITLE_CELL_PADDING_MM: f64 = 1.;
const TITLE_LABEL_SCALE: f64 = 0.65;
const TITLE_LINE_HEIGHT: f64 = 1.35;
pub const VIEW_KEYS: [&str; 6] = [
    "front",
    "top",
    "right",
    "isometric",
    "section-z",
    "detail-front",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkedText {
    Literal { text: String },
    DocumentName,
    DocumentProperty { key: String },
    ModelName,
    ModelProperty { key: String },
    ConfigurationName,
    MaterialName,
    SheetName,
    SheetNumber,
    SourceRevision,
    SourceFingerprint,
}
impl LinkedText {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Literal { .. } => "Literal text",
            Self::DocumentName => "Document name",
            Self::DocumentProperty { .. } => "Document property",
            Self::ModelName => "Model name",
            Self::ModelProperty { .. } => "Model property",
            Self::ConfigurationName => "Active configuration",
            Self::MaterialName => "Material",
            Self::SheetName => "Sheet name",
            Self::SheetNumber => "Sheet number / count",
            Self::SourceRevision => "Source revision",
            Self::SourceFingerprint => "Source fingerprint",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TitleCell {
    pub id: Id,
    pub label: String,
    pub value: LinkedText,
    /// Bottom-left X/Y and width/height relative to the title block, in mm.
    pub rect_mm: [f64; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TitleBlock {
    pub size_mm: [f64; 2],
    pub text_size_mm: f64,
    pub cells: Vec<TitleCell>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Zones {
    pub columns: usize,
    pub rows: usize,
    pub band_mm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SheetFormat {
    pub border: bool,
    /// Left, bottom, right and top paper margins, in mm.
    pub margins_mm: [f64; 4],
    pub zones: Option<Zones>,
    pub title_block: Option<TitleBlock>,
}
impl Default for SheetFormat {
    fn default() -> Self {
        let text_size_mm = 2.8_f64;
        // Keep a label and three wrapped value lines readable without relying
        // on a document name fitting on one line. Share the renderer's leading
        // and padding, and round up so the default never clips at a cell edge.
        let row_height = (text_size_mm * (TITLE_LABEL_SCALE + 3.) * TITLE_LINE_HEIGHT
            + 2. * TITLE_CELL_PADDING_MM)
            .ceil();
        let cells = [
            (
                "TITLE",
                LinkedText::ModelName,
                [0., row_height * 2., 180., row_height],
            ),
            (
                "DOCUMENT",
                LinkedText::DocumentName,
                [0., row_height, 90., row_height],
            ),
            (
                "PART NUMBER",
                LinkedText::ModelProperty {
                    key: "part_number".into(),
                },
                [90., row_height, 90., row_height],
            ),
            (
                "REVISION",
                LinkedText::DocumentProperty {
                    key: "revision".into(),
                },
                [0., 0., 60., row_height],
            ),
            (
                "CONFIGURATION",
                LinkedText::ConfigurationName,
                [60., 0., 60., row_height],
            ),
            (
                "SHEET",
                LinkedText::SheetNumber,
                [120., 0., 60., row_height],
            ),
        ]
        .into_iter()
        .map(|(label, value, rect_mm)| TitleCell {
            id: crate::id(),
            label: label.into(),
            value,
            rect_mm,
        })
        .collect();
        Self {
            border: true,
            margins_mm: [10.; 4],
            zones: Some(Zones {
                columns: 4,
                rows: 4,
                band_mm: 5.,
            }),
            title_block: Some(TitleBlock {
                size_mm: [180., row_height * 3.],
                text_size_mm,
                cells,
            }),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewAlignmentAxis {
    #[default]
    Free,
    /// Keep projected model origins on the same horizontal paper line.
    Horizontal,
    /// Keep projected model origins on the same vertical paper line.
    Vertical,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ViewAlignment {
    pub parent: String,
    #[serde(default)]
    pub axis: ViewAlignmentAxis,
    #[serde(default)]
    pub shared_scale: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ViewLayout {
    /// Bottom-left X/Y and width/height of the projection area, in paper mm.
    pub rect_mm: [f64; 4],
    /// Paper mm / model mm. None retains fitting to the saved rectangle.
    pub scale: Option<f64>,
    /// Saved source relationship; resolved positions are disposable projection data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<ViewAlignment>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SheetTemplate {
    pub version: u32,
    pub name: String,
    pub paper_mm: [f64; 2],
    pub format: SheetFormat,
    pub views: Vec<String>,
    pub annotations: Vec<String>,
    pub view_layouts: BTreeMap<String, ViewLayout>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_display_modes: BTreeMap<String, crate::drawing_visibility::DisplayMode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_visibility_geometry: BTreeMap<String, crate::drawing_visibility::GeometryMode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub title_block: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DimensionKind {
    #[default]
    Aligned,
    Horizontal,
    Vertical,
    /// Shortest projected distance between two parallel exact line carriers.
    Perpendicular,
    Diameter,
    Radius,
    Angular,
    HoleCallout,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    #[default]
    Millimeters,
    Inches,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tolerance {
    #[default]
    None,
    Symmetric {
        value: Expr,
    },
    Deviation {
        lower: Expr,
        upper: Expr,
    },
    Limits {
        lower: Expr,
        upper: Expr,
    },
    Basic,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointReference {
    /// A proved circular carrier center; it follows the native curve and occurrence.
    CurveCenter { curve: Box<CurveReference> },
    SketchPoint {
        sketch: NodeRef,
        point: Id,
        #[serde(default)]
        instance_path: Vec<Id>,
    },
    EdgePoint {
        edge: TopologyRef,
        #[serde(default)]
        body: Option<NodeRef>,
        parameter: f64,
        #[serde(default)]
        instance_path: Vec<Id>,
    },
    FaceOrigin {
        face: TopologyRef,
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        instance_path: Vec<Id>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveReference {
    SketchMember {
        sketch: NodeRef,
        member: Id,
        #[serde(default)]
        instance_path: Vec<Id>,
    },
    ModelEdge {
        edge: TopologyRef,
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        instance_path: Vec<Id>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DimensionOptions {
    #[serde(default)]
    pub kind: DimensionKind,
    #[serde(default)]
    pub first: Option<PointReference>,
    #[serde(default)]
    pub second: Option<PointReference>,
    #[serde(default)]
    pub third: Option<PointReference>,
    #[serde(default)]
    pub third_point: [f64; 3],
    #[serde(default)]
    pub curve: Option<CurveReference>,
    /// A second line carrier preserves edge-to-edge measurement intent instead
    /// of measuring the incidental pointer parameters used to select the edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_curve: Option<CurveReference>,
    #[serde(default)]
    pub hole_feature: Option<NodeRef>,
    #[serde(default)]
    pub tolerance: Tolerance,
    #[serde(default)]
    pub units: Units,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default)]
    pub reference: bool,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}
fn precision() -> usize {
    3
}
impl Default for DimensionOptions {
    fn default() -> Self {
        Self {
            kind: DimensionKind::Aligned,
            first: None,
            second: None,
            third: None,
            third_point: [0.; 3],
            curve: None,
            second_curve: None,
            hole_feature: None,
            tolerance: Tolerance::None,
            units: Units::Millimeters,
            precision: 3,
            reference: false,
            prefix: String::new(),
            suffix: String::new(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Anchor {
    pub point: [f64; 3],
    #[serde(default)]
    pub reference: Option<PointReference>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeometricCharacteristic {
    #[default]
    Position,
    Flatness,
    Straightness,
    Circularity,
    Cylindricity,
    Parallelism,
    Perpendicularity,
    Angularity,
    ProfileLine,
    ProfileSurface,
    CircularRunout,
    TotalRunout,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialCondition {
    #[default]
    Regardless,
    Maximum,
    Least,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationKind {
    RevisionCloud {
        target: Anchor,
        cloud: Box<crate::drawing_revision::Cloud>,
    },
    DatumTarget {
        target: Anchor,
        symbol: Box<crate::drawing_gdt::DatumTarget>,
    },
    GeometricToleranceFrame {
        target: Anchor,
        frame: Box<crate::drawing_gdt::ToleranceFrame>,
    },
    DimensionGroup {
        group: Box<crate::drawing_dimension_groups::Group>,
    },
    SurfaceFinish {
        target: Anchor,
        symbol: SurfaceFinish,
    },
    WeldSymbol {
        target: Anchor,
        symbol: WeldSymbol,
    },
    Leader {
        target: Anchor,
        text: String,
    },
    Datum {
        target: Anchor,
        identifier: String,
    },
    GeometricTolerance {
        target: Anchor,
        characteristic: GeometricCharacteristic,
        value: Expr,
        #[serde(default)]
        diameter: bool,
        #[serde(default)]
        material: MaterialCondition,
        #[serde(default)]
        datums: Vec<String>,
    },
    CenterMark {
        curve: CurveReference,
        extension_mm: f64,
    },
    CenterLine {
        first: Anchor,
        second: Box<Anchor>,
        extension_mm: f64,
    },
    CutListBalloon {
        target: Anchor,
        table: Id,
        row_key: String,
        source: crate::cut_list::Source,
        #[serde(default)]
        show_quantity: bool,
    },
    Balloon {
        target: Anchor,
        table: Id,
        #[serde(default)]
        row_key: Option<String>,
        definition: Id,
        #[serde(default)]
        configuration: Option<Id>,
        #[serde(default)]
        show_quantity: bool,
    },
}
impl AnnotationKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RevisionCloud { .. } => "Revision cloud",
            Self::DimensionGroup { .. } => "Dimension group",
            Self::DatumTarget { .. } => "Datum target",
            Self::GeometricToleranceFrame { .. } => "Tolerance frame",
            Self::SurfaceFinish { .. } => "Surface finish",
            Self::WeldSymbol { .. } => "Weld symbol",
            Self::Leader { .. } => "Leader note",
            Self::Datum { .. } => "Datum",
            Self::GeometricTolerance { .. } => "Geometric tolerance",
            Self::CenterMark { .. } => "Center mark",
            Self::CenterLine { .. } => "Centerline",
            Self::Balloon { .. } => "BOM balloon",
            Self::CutListBalloon { .. } => "Cut-list balloon",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub id: Id,
    pub view: String,
    #[serde(default)]
    pub offset_mm: [f64; 2],
    /// Ordered paper-mm elbow offsets relative to the current model attachment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leader_points_mm: Vec<[f64; 2]>,
    pub kind: AnnotationKind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    /// Occluded model edges use short engineering dashes, separately from
    /// center/reference/ghost strokes carried by the legacy `dashed` flag.
    HiddenPolyline {
        points: Vec<[f64; 2]>,
    },
    HiddenSpline {
        curve: crate::sketch_edit::Curve2,
    },
    Polyline {
        points: Vec<[f64; 2]>,
        dashed: bool,
    },
    Spline {
        curve: crate::sketch_edit::Curve2,
        #[serde(default)]
        dashed: bool,
    },
    Text {
        at: [f64; 2],
        size: f64,
        text: String,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewProjection {
    pub rect_mm: [f64; 4],
    pub model_center: [f64; 2],
    pub scale: f64,
}
impl ViewProjection {
    pub fn paper_point(&self, key: &str, point: [f64; 3]) -> Result<[f64; 2]> {
        Ok(self.paper_projected(project(key, point)?))
    }
    pub fn paper_projected(&self, point: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|axis| {
            self.rect_mm[axis]
                + self.rect_mm[axis + 2] * 0.5
                + (point[axis] - self.model_center[axis]) * self.scale
        })
    }
}
/// Rendering fidelity travels with a view, separately from its production paper vectors.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewFidelity {
    pub description: String,
    /// True only when occluded strokes are omitted, not when shown dashed.
    pub hidden_line_removal: bool,
    pub approximate: bool,
    #[serde(default)]
    pub visibility: crate::drawing_visibility::Fidelity,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrawingSourceInfo {
    pub document: Id,
    pub revision: u64,
    pub fingerprint: String,
    pub backend: String,
}
impl DrawingSourceInfo {
    fn current(doc: &Document, evaluation: &Evaluation) -> Self {
        Self {
            document: doc.id.clone(),
            revision: doc.revision,
            fingerprint: doc.fingerprint(),
            backend: evaluation.identity.backend.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sheet {
    pub id: Id,
    pub name: String,
    pub paper_mm: [f64; 2],
    pub primitives: Vec<Primitive>,
    /// Native implicit/parameter carriers and certified model-space error bounds
    /// retained separately from their finite paper rendering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_traces: Vec<NativeTraceBinding>,
    pub views: BTreeMap<String, ViewLayout>,
    #[serde(default)]
    pub projections: BTreeMap<String, ViewProjection>,
    #[serde(default)]
    pub view_fidelity: BTreeMap<String, ViewFidelity>,
    #[serde(default)]
    pub source_info: DrawingSourceInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_state: Option<crate::drawing_display_states::Info>,
    #[serde(default)]
    pub groups: Vec<crate::drawing_interaction::PrimitiveGroup>,
    /// Preview-only diagnostics; production export still rejects these source errors.
    #[serde(default)]
    pub diagnostics: BTreeMap<Id, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeTraceBinding {
    pub view: String,
    /// Half-open range into the paper primitive array, including hidden strokes.
    pub primitive_range: [usize; 2],
    pub geometry: crate::drawing_visibility::NativeTrace,
}
#[derive(Clone)]
struct Placement {
    key: String,
    matrix: [[f64; 4]; 4],
}
fn invalid(s: impl ToString) -> CadError {
    CadError::Invalid(format!("Drawing: {}", s.to_string()))
}
pub fn view_keys(sheet: &Drawing) -> Vec<String> {
    if sheet.views.is_empty() {
        vec!["front".into()]
    } else {
        sheet.views.clone()
    }
}
pub fn view_id(sheet: &Drawing, key: &str) -> Id {
    format!("{}::view:{key}", sheet.id)
}
/// Drawable border interior, accounting for the optional zone band.
pub fn paper_bounds(sheet: &Drawing) -> [[f64; 2]; 2] {
    let margins = sheet
        .format
        .as_ref()
        .map(|f| f.margins_mm)
        .unwrap_or([8.; 4]);
    let band = sheet
        .format
        .as_ref()
        .and_then(|f| f.zones.as_ref())
        .map(|z| z.band_mm)
        .unwrap_or(0.);
    [
        [margins[0] + band, margins[1] + band],
        [
            sheet.paper_mm[0] - margins[2] - band,
            sheet.paper_mm[1] - margins[3] - band,
        ],
    ]
}
pub fn title_bounds(sheet: &Drawing) -> Option<[[f64; 2]; 2]> {
    let block = sheet.format.as_ref()?.title_block.as_ref()?;
    let bounds = paper_bounds(sheet);
    Some([
        [bounds[1][0] - block.size_mm[0], bounds[0][1]],
        [bounds[1][0], bounds[0][1] + block.size_mm[1]],
    ])
}
pub fn default_table_position(sheet: &Drawing) -> [f64; 2] {
    let bounds = paper_bounds(sheet);
    [
        bounds[0][0] + 2.,
        title_bounds(sheet)
            .map(|b| b[1][1] + 4.)
            .unwrap_or(bounds[0][1] + 2.),
    ]
}
fn rectangles_overlap(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
    a[0][0] < b[1][0] && b[0][0] < a[1][0] && a[0][1] < b[1][1] && b[0][1] < a[1][1]
}
fn text_valid(text: &str) -> bool {
    text.len() <= 65536 && !text.chars().any(|c| c.is_control() && c != '\n')
}
pub fn validate_format(format: &SheetFormat, paper: [f64; 2]) -> Result<()> {
    if paper
        .iter()
        .any(|v| !v.is_finite() || !(50. ..=10000.).contains(v))
        || format.margins_mm.iter().any(|v| !v.is_finite() || *v < 2.)
    {
        return Err(invalid(
            "Sheet formats require finite 50–10000 mm paper dimensions and margins of at least 2 mm",
        ));
    }
    let band = if let Some(zones) = &format.zones {
        if !format.border
            || !(1..=100).contains(&zones.columns)
            || !(1..=26).contains(&zones.rows)
            || !zones.band_mm.is_finite()
            || zones.band_mm < 3.
        {
            return Err(invalid(
                "Zones require a border, 1–100 numbered columns, 1–26 lettered rows and a band of at least 3 mm",
            ));
        }
        zones.band_mm
    } else {
        0.
    };
    let inner = [
        paper[0] - format.margins_mm[0] - format.margins_mm[2] - 2. * band,
        paper[1] - format.margins_mm[1] - format.margins_mm[3] - 2. * band,
    ];
    if inner.iter().any(|v| !v.is_finite() || *v < 25.) {
        return Err(invalid(
            "Margins and zone bands leave too little drawing space",
        ));
    }
    if let Some(zones) = &format.zones
        && (inner[0] / (zones.columns as f64) < 4. || inner[1] / (zones.rows as f64) < 4.)
    {
        return Err(invalid("Zone spacing must be at least 4 mm"));
    }
    if let Some(block) = &format.title_block {
        if block.size_mm.iter().any(|v| !v.is_finite() || *v < 8.)
            || block.size_mm[0] > inner[0]
            || block.size_mm[1] + 25. > inner[1]
            || !block.text_size_mm.is_finite()
            || !(0.5..=10.).contains(&block.text_size_mm)
            || block.cells.len() > 128
        {
            return Err(invalid(
                "Title blocks need finite dimensions inside the border, drawing space above them, 0.5–10 mm text and at most 128 cells",
            ));
        }
        let mut ids = BTreeSet::new();
        for (index, cell) in block.cells.iter().enumerate() {
            let [x, y, width, height] = cell.rect_mm;
            if cell.id.is_empty()
                || !ids.insert(&cell.id)
                || !text_valid(&cell.label)
                || cell.rect_mm.iter().any(|v| !v.is_finite())
                || x < 0.
                || y < 0.
                || width < 4.
                || height < 4.
                || x + width > block.size_mm[0] + 1e-9
                || y + height > block.size_mm[1] + 1e-9
            {
                return Err(invalid(
                    "Title cells need distinct identities, finite nonnegative positions and positive rectangles inside their block",
                ));
            }
            match &cell.value {
                LinkedText::Literal { text } if !text_valid(text) => {
                    return Err(invalid(
                        "Title text contains unsupported control characters or exceeds 64 KiB",
                    ));
                }
                LinkedText::DocumentProperty { key } | LinkedText::ModelProperty { key }
                    if key.trim().is_empty() || !text_valid(key) =>
                {
                    return Err(invalid("Linked property keys must be nonempty text"));
                }
                _ => {}
            }
            let bounds = [[x, y], [x + width, y + height]];
            if block.cells[..index].iter().any(|other| {
                let [x, y, width, height] = other.rect_mm;
                // Shared cell borders are allowed; positive area overlap is not.
                rectangles_overlap(
                    [
                        [bounds[0][0] + 1e-9, bounds[0][1] + 1e-9],
                        [bounds[1][0] - 1e-9, bounds[1][1] - 1e-9],
                    ],
                    [[x, y], [x + width, y + height]],
                )
            }) {
                return Err(invalid("Title-cell rectangles overlap"));
            }
        }
    }
    Ok(())
}
pub fn linked_text(doc: &Document, sheet: &Drawing, value: &LinkedText) -> Result<String> {
    let config = doc.active_configurations.get(&sheet.definition);
    let result = match value {
        LinkedText::Literal { text } => text.clone(),
        LinkedText::DocumentName => doc.name.clone(),
        LinkedText::DocumentProperty { key } => {
            doc.properties.get(key).cloned().unwrap_or_default()
        }
        LinkedText::ModelName => doc
            .parts
            .get(&sheet.definition)
            .map(|p| p.name.clone())
            .or_else(|| {
                doc.assemblies
                    .get(&sheet.definition)
                    .map(|a| a.name.clone())
            })
            .ok_or_else(|| invalid("Title block model scope is missing"))?,
        LinkedText::ModelProperty { key } => {
            crate::model_properties::value(doc, &sheet.definition, config.map(String::as_str), key)?
        }
        LinkedText::ConfigurationName => config
            .and_then(|key| {
                crate::configuration::names(doc, &sheet.definition)
                    .get(key)
                    .cloned()
            })
            .unwrap_or_else(|| "Default".into()),
        LinkedText::MaterialName => {
            let part = doc
                .parts
                .get(&sheet.definition)
                .ok_or_else(|| invalid("A material title field requires a part sheet scope"))?;
            let configured = config
                .map(|c| crate::configuration::resolve(doc, &sheet.definition, c))
                .transpose()?;
            let material = configured
                .as_ref()
                .and_then(|c| c.overrides.material.as_ref())
                .unwrap_or(&part.material);
            doc.materials
                .get(material)
                .map(|m| m.name.clone())
                .ok_or_else(|| invalid("Title material is missing"))?
        }
        LinkedText::SheetName => sheet.name.clone(),
        LinkedText::SheetNumber => {
            // Exact saved identities take precedence over the derived-page suffix:
            // Advanced Source may legitimately contain that text in a master ID.
            let (master, page) = if doc.drawings.iter().any(|saved| saved.id == sheet.id) {
                (sheet.id.as_str(), 0)
            } else {
                sheet
                    .id
                    .rsplit_once("::continuation:")
                    .and_then(|(id, index)| {
                        index
                            .parse::<u32>()
                            .ok()
                            .filter(|page| {
                                doc.drawings.iter().any(|saved| {
                                    saved.id == id
                                        && *page < crate::drawing_tables::page_count(saved)
                                })
                            })
                            .map(|page| (id, page))
                    })
                    .unwrap_or((sheet.id.as_str(), 0))
            };
            let preceding: u64 = if doc.drawings.iter().any(|saved| saved.id == master) {
                doc.drawings
                    .iter()
                    .take_while(|saved| saved.id != master)
                    .map(|saved| u64::from(crate::drawing_tables::page_count(saved)))
                    .sum()
            } else {
                0
            };
            let total: u64 = doc
                .drawings
                .iter()
                .map(|saved| u64::from(crate::drawing_tables::page_count(saved)))
                .sum();
            format!("{} / {}", preceding + u64::from(page) + 1, total.max(1))
        }
        LinkedText::SourceRevision => doc.revision.to_string(),
        LinkedText::SourceFingerprint => doc.fingerprint(),
    };
    if !text_valid(&result) {
        return Err(invalid(
            "Linked title text contains unsupported control characters or exceeds 64 KiB",
        ));
    }
    Ok(result)
}
fn owned_ids(sheet: &Drawing) -> BTreeSet<Id> {
    std::iter::once(sheet.id.clone())
        .chain(
            (1..crate::drawing_tables::page_count(sheet))
                .map(|page| format!("{}::continuation:{page}", sheet.id)),
        )
        .chain(view_keys(sheet).iter().map(|key| view_id(sheet, key)))
        .chain(sheet.dimensions.iter().map(|d| d.id.clone()))
        .chain(sheet.entities.iter().map(|a| a.id.clone()))
        .chain(sheet.tables.iter().flat_map(|t| {
            std::iter::once(t.id.clone())
                .chain(crate::drawing_tables::owned_row_ids(t))
                .chain(t.segments.iter().map(|segment| segment.id.clone()))
                .chain(match &t.kind {
                    crate::drawing_tables::TableKind::Revisions { rows } => {
                        rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()
                    }
                    _ => vec![],
                })
        }))
        .chain(
            sheet
                .format
                .iter()
                .flat_map(|f| f.title_block.iter())
                .flat_map(|b| b.cells.iter().map(|c| c.id.clone())),
        )
        .collect()
}
fn guard_retired(doc: &Document, owner: &str, retired: &BTreeSet<Id>) -> Result<()> {
    if let Some(comment) = doc.comments.iter().find(|c| retired.contains(&c.target)) {
        return Err(invalid(format!(
            "Retired sheet items are referenced by review comment {}; remove or reattach that comment first",
            comment.id
        )));
    }
    if doc
        .drawings
        .iter()
        .filter(|s| s.id != owner)
        .flat_map(|s| &s.entities)
        .any(|a| matches!(&a.kind,AnnotationKind::Balloon{table,..}|AnnotationKind::CutListBalloon{table,..} if retired.contains(table)))
    {
        return Err(invalid(
            "A retired table is referenced by a balloon on another sheet",
        ));
    }
    Ok(())
}
pub fn validate_document(doc: &Document) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for sheet in &doc.drawings {
        validate_definition(sheet)?;
        for table in &sheet.tables {
            crate::drawing_tables::validate_sources(doc, table)?;
        }
        let mut source_scope = doc.clone();
        source_scope.root = sheet.definition.clone();
        for entity in &sheet.entities {
            if let AnnotationKind::CutListBalloon { source, .. } = &entity.kind {
                crate::cut_list::validate_source(&source_scope, source)?;
            }
        }

        if !doc.parts.contains_key(&sheet.definition)
            && !doc.assemblies.contains_key(&sheet.definition)
        {
            return Err(invalid("Sheet model scope is missing"));
        }
        if let Some(key) = &sheet.exploded_view
            && (sheet.definition != doc.root
                || !doc
                    .assemblies
                    .get(&sheet.definition)
                    .is_some_and(|a| a.exploded_views.contains_key(key)))
        {
            return Err(invalid(
                "Exploded sheet source must be a saved view on the current root assembly",
            ));
        }
        if sheet.display_state.is_some() {
            crate::drawing_display_states::scope(doc, sheet)?;
        }
        if !names.insert(sheet.name.trim().to_lowercase())
            || owned_ids(sheet).iter().any(|id| !ids.insert(id.clone()))
        {
            return Err(invalid(
                "Drawing sheets need distinct names and globally distinct sheet, view, annotation, table and title-cell identities",
            ));
        }
    }
    Ok(())
}
pub fn put_sheet(doc: &mut Document, sheet: Drawing) -> Result<()> {
    validate_definition(&sheet)?;
    if let Some(old) = doc.drawings.iter().find(|d| d.id == sheet.id) {
        let retired = owned_ids(old)
            .difference(&owned_ids(&sheet))
            .cloned()
            .collect();
        guard_retired(doc, &sheet.id, &retired)?;
    }
    let mut candidate = doc.clone();
    if let Some(old) = candidate.drawings.iter_mut().find(|d| d.id == sheet.id) {
        *old = sheet;
    } else {
        candidate.drawings.push(sheet);
    }
    validate_document(&candidate)?;
    *doc = candidate;
    Ok(())
}
pub fn new_sheet(doc: &Document, definition: &str, name: String) -> Result<Drawing> {
    if !doc.parts.contains_key(definition) && !doc.assemblies.contains_key(definition) {
        return Err(invalid("Sheet model scope is missing"));
    }
    let sheet = Drawing {
        id: crate::id(),
        name,
        definition: definition.into(),
        views: vec!["front".into(), "top".into(), "right".into()],
        annotations: vec!["All dimensions in millimeters".into()],
        paper_mm: [297., 210.],
        dimensions: vec![],
        title_block: BTreeMap::new(),
        entities: vec![],
        tables: vec![],
        format: Some(SheetFormat::default()),
        view_layouts: BTreeMap::new(),
        view_display_modes: ["front", "top", "right"]
            .map(|key| {
                (
                    key.into(),
                    crate::drawing_visibility::DisplayMode::HiddenLinesRemoved,
                )
            })
            .into_iter()
            .collect(),
        view_visibility_geometry: BTreeMap::new(),
        exploded_view: None,
        display_state: None,
    };
    validate_definition(&sheet)?;
    Ok(sheet)
}
pub fn move_sheet(doc: &mut Document, key: &str, index: usize) -> Result<()> {
    if index >= doc.drawings.len() {
        return Err(invalid(
            "Sheet destination is outside the saved sheet order",
        ));
    }
    let source = doc
        .drawings
        .iter()
        .position(|d| d.id == key)
        .ok_or_else(|| invalid("Sheet is missing"))?;
    let sheet = doc.drawings.remove(source);
    doc.drawings.insert(index, sheet);
    Ok(())
}
pub fn delete_sheet(doc: &mut Document, key: &str) -> Result<()> {
    let sheet = doc
        .drawings
        .iter()
        .find(|d| d.id == key)
        .ok_or_else(|| invalid("Sheet is missing"))?;
    guard_retired(doc, key, &owned_ids(sheet))?;
    doc.drawings.retain(|d| d.id != key);
    Ok(())
}
pub fn duplicate_sheet(doc: &mut Document, key: &str, name: String) -> Result<Id> {
    let mut sheet = doc
        .drawings
        .iter()
        .find(|d| d.id == key)
        .cloned()
        .ok_or_else(|| invalid("Sheet is missing"))?;
    sheet.id = crate::id();
    sheet.name = name;
    for dimension in &mut sheet.dimensions {
        dimension.id = crate::id();
    }
    let mut tables = BTreeMap::new();
    let mut revision_rows = BTreeMap::new();
    for table in &mut sheet.tables {
        let next = crate::id();
        tables.insert(table.id.clone(), next.clone());
        table.id = next;
        for segment in &mut table.segments {
            segment.id = crate::id();
        }
        if let crate::drawing_tables::TableKind::Revisions { rows } = &mut table.kind {
            for row in rows {
                let next = crate::id();
                revision_rows.insert(row.id.clone(), next.clone());
                row.id = next;
            }
        }
    }
    for annotation in &mut sheet.entities {
        annotation.id = crate::id();
        if let AnnotationKind::RevisionCloud { cloud, .. } = &mut annotation.kind {
            for vertex in &mut cloud.vertices {
                vertex.id = crate::id();
            }
            if let Some(link) = &mut cloud.revision {
                link.table = tables.get(&link.table).cloned().ok_or_else(|| {
                    invalid("The duplicated cloud revision table is outside its sheet")
                })?;
                link.row = revision_rows.get(&link.row).cloned().ok_or_else(|| {
                    invalid("The duplicated cloud revision row is outside its sheet")
                })?;
            }
        }
        if let AnnotationKind::GeometricToleranceFrame { frame, .. } = &mut annotation.kind {
            for row in &mut frame.rows {
                row.id = crate::id();
            }
        }
        if let AnnotationKind::DimensionGroup { group } = &mut annotation.kind {
            for target in &mut group.targets {
                target.id = crate::id();
            }
        }
        if let AnnotationKind::Balloon { table, .. }
        | AnnotationKind::CutListBalloon { table, .. } = &mut annotation.kind
        {
            *table = tables
                .get(table)
                .cloned()
                .ok_or_else(|| invalid("Duplicated balloon table is outside its sheet"))?;
        }
    }
    if let Some(format) = &mut sheet.format {
        refresh_format_ids(format);
    }
    let next = sheet.id.clone();
    let source = doc.drawings.iter().position(|d| d.id == key).unwrap();
    let mut candidate = doc.clone();
    put_sheet(&mut candidate, sheet)?;
    move_sheet(&mut candidate, &next, source + 1)?;
    *doc = candidate;
    Ok(next)
}
fn refresh_format_ids(format: &mut SheetFormat) {
    if let Some(block) = &mut format.title_block {
        for cell in &mut block.cells {
            cell.id = crate::id();
        }
    }
}
pub fn set_views(sheet: &mut Drawing, views: Vec<String>) -> Result<()> {
    if views.is_empty() {
        return Err(invalid("Keep at least one view on the sheet"));
    }
    if sheet.dimensions.iter().any(|d| !views.contains(&d.view))
        || sheet.entities.iter().any(|a| !views.contains(&a.view))
        || sheet.tables.iter().any(|t|matches!(&t.kind,crate::drawing_tables::TableKind::Holes {view,..} if !views.contains(view)))
    {
        return Err(invalid(
            "Move or delete dimensions and annotations before removing their view",
        ));
    }
    if sheet.view_layouts.values().any(|layout| {
        layout
            .alignment
            .as_ref()
            .is_some_and(|alignment| !views.contains(&alignment.parent))
    }) {
        return Err(invalid(
            "Break or reparent aligned views before removing their parent view",
        ));
    }
    let convention = sheet.view_display_modes.values().next().copied();
    let geometry = sheet.view_visibility_geometry.values().next().copied();
    for key in &views {
        if !sheet.views.contains(key)
            && key != "section-z"
            && let Some(mode) = convention
        {
            sheet.view_display_modes.insert(key.clone(), mode);
        }
        if !sheet.views.contains(key)
            && key != "section-z"
            && let Some(mode) = geometry
        {
            sheet.view_visibility_geometry.insert(key.clone(), mode);
        }
    }
    sheet.views = views;
    sheet
        .view_layouts
        .retain(|key, _| sheet.views.contains(key));
    sheet
        .view_display_modes
        .retain(|key, _| sheet.views.contains(key));
    sheet
        .view_visibility_geometry
        .retain(|key, _| sheet.views.contains(key));
    Ok(())
}
impl SheetTemplate {
    pub fn from_sheet(sheet: &Drawing) -> Self {
        Self {
            version: FORMAT_VERSION,
            name: sheet.name.clone(),
            paper_mm: sheet.paper_mm,
            format: sheet.format.clone().unwrap_or_default(),
            views: view_keys(sheet),
            annotations: sheet.annotations.clone(),
            view_layouts: sheet.view_layouts.clone(),
            view_display_modes: sheet.view_display_modes.clone(),
            view_visibility_geometry: sheet.view_visibility_geometry.clone(),
            title_block: sheet.title_block.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=FORMAT_VERSION).contains(&self.version)
            || self.name.trim().is_empty()
            || !text_valid(&self.name)
            || self.views.is_empty()
        {
            return Err(invalid("Unsupported or unnamed drawing sheet template"));
        }
        validate_format(&self.format, self.paper_mm)?;
        let sheet = Drawing {
            id: "template".into(),
            name: self.name.clone(),
            definition: "template".into(),
            views: self.views.clone(),
            annotations: self.annotations.clone(),
            paper_mm: self.paper_mm,
            dimensions: vec![],
            title_block: self.title_block.clone(),
            entities: vec![],
            tables: vec![],
            format: Some(self.format.clone()),
            view_layouts: self.view_layouts.clone(),
            view_display_modes: self.view_display_modes.clone(),
            view_visibility_geometry: self.view_visibility_geometry.clone(),
            exploded_view: None,
            display_state: None,
        };
        validate_definition(&sheet)
    }
    /// Templates are copied into native source; no external mutable link is retained.
    pub fn instantiate(&self, doc: &Document, definition: &str, name: String) -> Result<Drawing> {
        self.validate()?;
        let mut sheet = new_sheet(doc, definition, name)?;
        sheet.paper_mm = self.paper_mm;
        sheet.views = self.views.clone();
        sheet.annotations = self.annotations.clone();
        sheet.view_layouts = self.view_layouts.clone();
        sheet.view_display_modes = self.view_display_modes.clone();
        sheet.view_visibility_geometry = self.view_visibility_geometry.clone();
        sheet.title_block = self.title_block.clone();
        let mut format = self.format.clone();
        refresh_format_ids(&mut format);
        sheet.format = Some(format);
        validate_definition(&sheet)?;
        Ok(sheet)
    }
    pub fn apply_format(&self, sheet: &mut Drawing) -> Result<()> {
        self.validate()?;
        let mut candidate = sheet.clone();
        candidate.paper_mm = self.paper_mm;
        let mut format = self.format.clone();
        refresh_format_ids(&mut format);
        candidate.format = Some(format);
        validate_definition(&candidate)?;
        *sheet = candidate;
        Ok(())
    }
}
fn transform(p: [f64; 3], m: [[f64; 4]; 4]) -> [f64; 3] {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}
pub fn scoped_document(doc: &Document, _e: &Evaluation, sheet: &Drawing) -> Document {
    let mut scoped = doc.clone();
    scoped.root = sheet.definition.clone();
    scoped
}
fn exploded_stage<'a>(
    doc: &Document,
    e: &'a Evaluation,
    sheet: &Drawing,
) -> Result<Option<&'a crate::exploded_views::Stage>> {
    let Some(key) = &sheet.exploded_view else {
        return Ok(None);
    };
    if sheet.definition != doc.root || !doc.assemblies.contains_key(&sheet.definition) {
        return Err(invalid(
            "Exploded drawing sheets currently require the current root assembly scope",
        ));
    }
    let cached = e
        .exploded_views
        .get(key)
        .ok_or_else(|| invalid("Selected exploded view has no current compatible evaluation"))?;
    if cached.version != crate::exploded_views::CONTRACT_VERSION
        || cached.assembly != sheet.definition
        || cached.view != *key
        || cached.configuration != doc.active_configurations.get(&sheet.definition).cloned()
    {
        return Err(invalid(
            "Selected exploded view cache belongs to another assembly or configuration",
        ));
    }
    Ok(Some(cached.stages.last().ok_or_else(|| {
        invalid("Exploded view has no evaluated stage")
    })?))
}
pub fn sheet_instances(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
) -> Result<Vec<crate::PlacedPart>> {
    if let Some(stage) = exploded_stage(doc, e, sheet)? {
        return Ok(stage.instances.clone());
    }
    if sheet.definition == doc.root {
        Ok(e.instances
            .iter()
            .cloned()
            .chain(e.assembly_welds.values().map(|weld| weld.placement.clone()))
            .collect())
    } else {
        let scoped = scoped_document(doc, e, sheet);
        let mut instances = crate::configuration::placed_parts(&scoped, Some(&e.assemblies))?;
        if scoped.assemblies.contains_key(&sheet.definition) {
            let occurrences = crate::assembly_welds::occurrences(doc, &e.assemblies)?;
            let configuration = doc.active_configurations.get(&sheet.definition);
            if let Some(occurrence) = occurrences.iter().find(|occurrence| {
                occurrence.definition == sheet.definition
                    && occurrence.configuration.as_ref() == configuration
            }) {
                let inverse = occurrence
                    .matrix
                    .try_inverse()
                    .ok_or_else(|| invalid("Drawing assembly occurrence pose is singular"))?;
                for weld in e
                    .assembly_welds
                    .values()
                    .filter(|weld| weld.placement.path.starts_with(&occurrence.path))
                {
                    let mut placement = weld.placement.clone();
                    let matrix = nalgebra::Matrix4::from_fn(|r, c| placement.matrix[r][c]);
                    let matrix = inverse * matrix;
                    placement.matrix =
                        std::array::from_fn(|r| std::array::from_fn(|c| matrix[(r, c)]));
                    placement.path = placement.path[occurrence.path.len()..].to_vec();
                    instances.push(placement);
                }
            } else if crate::assembly_welds::occurrences(&scoped, &e.assemblies)?
                .iter()
                .any(|occurrence| {
                    occurrence
                        .context
                        .assemblies
                        .get(&occurrence.definition)
                        .is_some_and(|assembly| {
                            assembly.weld_beads.values().any(|weld| !weld.suppressed)
                        })
                })
            {
                return Err(crate::CadError::Unsupported("A welded subassembly drawing needs a current evaluated occurrence of the same assembly configuration; activate or place that configuration before drawing its deposited metal".into()));
            }
        }
        Ok(instances)
    }
}
pub fn sheet_display_instances(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
) -> Result<Vec<crate::drawing_display_states::DisplayInstance>> {
    let scope = crate::drawing_display_states::scope(doc, sheet)?;
    Ok(crate::drawing_display_states::apply(
        scope.as_ref(),
        sheet_instances(doc, e, sheet)?,
    ))
}
fn placements(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    definition: &str,
    path: &[Id],
) -> Result<Vec<Placement>> {
    let path = if sheet.definition == definition && doc.parts.contains_key(definition) {
        &[][..]
    } else {
        path
    };
    let instances = sheet_instances(doc, e, sheet)?;
    let placements = instances
        .into_iter()
        .filter(|i| i.definition == definition && (path.is_empty() || i.path == path))
        .map(|i| Placement {
            key: if !i.mesh_key.is_empty() {
                i.mesh_key
            } else if let Some(config) = i.configuration {
                format!("{}@{config}", i.definition)
            } else {
                i.definition
            },
            matrix: i.matrix,
        })
        .collect::<Vec<_>>();
    if placements.is_empty() {
        return Err(invalid(
            "An annotation source is outside the drawing's component scope",
        ));
    }
    Ok(placements)
}
fn unique_placement(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    definition: &str,
    path: &[Id],
) -> Result<Placement> {
    let mut candidates = placements(doc, e, sheet, definition, path)?;
    if candidates.len() != 1 {
        return Err(invalid(
            "The annotation source has multiple component instances; select a specific instance",
        ));
    }
    Ok(candidates.remove(0))
}
pub(crate) fn table_placement(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    definition: &str,
    path: &[Id],
) -> Result<(String, [[f64; 4]; 4])> {
    let placement = unique_placement(doc, evaluation, sheet, definition, path)?;
    Ok((placement.key, placement.matrix))
}
fn body_matches(doc: &Document, actual: &Option<NodeRef>, required: &Option<NodeRef>) -> bool {
    let Some(required) = required else {
        return true;
    };
    let Some(actual) = actual else {
        return false;
    };
    if let Some(assembly) = doc.assemblies.get(&required.definition)
        && !assembly
            .weld_beads
            .get(&required.node)
            .is_some_and(|weld| !weld.suppressed)
    {
        return false;
    }
    let mut pending = vec![actual.clone()];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if reference == *required {
            return true;
        }
        if !visited.insert(reference.clone()) {
            continue;
        }
        if let Some(feature) = doc.parts.get(&reference.definition).and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == reference.node && !feature.suppressed)
        }) {
            pending.extend(feature.operation.consumed_outputs());
        }
    }
    false
}
fn resolve_edge<'a>(
    doc: &Document,
    e: &'a Evaluation,
    sheet: &Drawing,
    reference: &TopologyRef,
    body: &Option<NodeRef>,
    path: &[Id],
) -> Result<(&'a EdgeData, Placement)> {
    let place = unique_placement(
        doc,
        e,
        sheet,
        body.as_ref()
            .map_or(&reference.producer.definition, |body| &body.definition),
        path,
    )?;
    let mesh = e
        .meshes
        .get(&place.key)
        .ok_or_else(|| invalid("The referenced component has no current evaluated geometry"))?;
    if mesh.edges.len() > 1_048_576 {
        return Err(CadError::Unsupported(
            "Drawing edge source lookup exceeds 1048576 candidates".into(),
        ));
    }
    let wanted = crate::provenance::normalized_reference(reference)?;
    let mut candidates = Vec::new();
    for edge in &mesh.edges {
        if body_matches(doc, &edge.body, body)
            && crate::provenance::matches_normalized(&edge.reference, reference, &wanted)?
        {
            candidates.push(edge);
        }
    }
    if candidates.len() != 1 {
        return Err(invalid(format!(
            "Edge reference is {} ({} candidates); select a replacement",
            if candidates.is_empty() {
                "missing"
            } else {
                "ambiguous"
            },
            candidates.len()
        )));
    }
    Ok((candidates[0], place))
}
pub(crate) fn curve_point(curve: &crate::sketch_edit::Curve3, t: f64) -> Result<[f64; 3]> {
    let xy = crate::sketch_edit::Curve2 {
        control_points: curve.control_points.iter().map(|p| [p[0], p[1]]).collect(),
        weights: curve.weights.clone(),
        knots: curve.knots.clone(),
        degree: curve.degree,
    };
    let xz = crate::sketch_edit::Curve2 {
        control_points: curve.control_points.iter().map(|p| [p[0], p[2]]).collect(),
        ..xy.clone()
    };
    let a = crate::sketch_edit::nurbs_point(&xy, t)?;
    let b = crate::sketch_edit::nurbs_point(&xz, t)?;
    Ok([a[0], a[1], b[1]])
}
pub fn point(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    fallback: [f64; 3],
    reference: &Option<PointReference>,
) -> Result<[f64; 3]> {
    let Some(reference) = reference else {
        return Ok(fallback);
    };
    match reference {
        PointReference::CurveCenter { curve } => Ok(circular(doc, e, sheet, curve)?.center),
        PointReference::SketchPoint {
            sketch,
            point,
            instance_path,
        } => {
            let place = unique_placement(doc, e, sheet, &sketch.definition, instance_path)?;
            let geometry = e
                .sketches
                .get(&format!("{}::{}", place.key, sketch.node))
                .ok_or_else(|| invalid("Sketch reference is missing or suppressed"))?;
            if geometry.solved.as_ref().is_some_and(|source| {
                crate::sketch_transform::suppressed_points(source).contains(point)
            }) {
                return Err(invalid(
                    "Referenced sketch datum point is suppressed in its current configuration",
                ));
            }
            let p = geometry
                .points
                .get(point)
                .ok_or_else(|| invalid("Referenced sketch vertex no longer exists"))?;
            Ok(transform(
                std::array::from_fn(|i| {
                    geometry.plane.origin[i]
                        + geometry.plane.u[i] * p[0]
                        + geometry.plane.v[i] * p[1]
                }),
                place.matrix,
            ))
        }
        PointReference::EdgePoint {
            edge,
            body,
            parameter,
            instance_path,
        } => {
            if !parameter.is_finite() || !(0.0..=1.0).contains(parameter) {
                return Err(invalid("Edge parameter must be 0..1"));
            }
            let (edge, place) = resolve_edge(doc, e, sheet, edge, body, instance_path)?;
            let curve=edge.curve.as_ref().ok_or_else(||CadError::Unsupported("Drawing anchor requires an exact supported curve representation; display samples are not measurement data".into()))?;
            Ok(transform(curve_point(curve, *parameter)?, place.matrix))
        }
        PointReference::FaceOrigin {
            face,
            body,
            instance_path,
        } => {
            let place = unique_placement(
                doc,
                e,
                sheet,
                body.as_ref()
                    .map_or(&face.producer.definition, |body| &body.definition),
                instance_path,
            )?;
            let mesh = e
                .meshes
                .get(&place.key)
                .ok_or_else(|| invalid("No current geometry for face anchor"))?;
            if mesh.faces.len() > 1_048_576 {
                return Err(CadError::Unsupported(
                    "Drawing face source lookup exceeds 1048576 candidates".into(),
                ));
            }
            let wanted = crate::provenance::normalized_reference(face)?;
            let mut faces = Vec::new();
            for candidate in &mesh.faces {
                if body_matches(doc, &candidate.body, body)
                    && crate::provenance::matches_normalized(&candidate.reference, face, &wanted)?
                {
                    faces.push(candidate);
                }
            }
            if faces.len() != 1 {
                return Err(invalid(
                    "Face anchor is missing or ambiguous; select a replacement",
                ));
            }
            let plane = faces[0].plane.as_ref().ok_or_else(|| {
                CadError::Unsupported("Face plane-origin anchors require a planar face".into())
            })?;
            Ok(transform(plane.origin, place.matrix))
        }
    }
}
#[derive(Clone, Debug)]
pub struct CircularGeometry {
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub radius: f64,
    pub start: [f64; 3],
}
/// Exact, world-space straight carrier for associative edge separation.
#[derive(Clone, Debug)]
pub struct LinearGeometry {
    pub start: [f64; 3],
    pub end: [f64; 3],
}
pub fn linear(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    reference: &CurveReference,
) -> Result<LinearGeometry> {
    match reference {
        CurveReference::ModelEdge {
            edge,
            body,
            instance_path,
        } => {
            let (edge, place) = resolve_edge(doc, e, sheet, edge, body, instance_path)?;
            let curve = edge
                .curve
                .as_ref()
                .ok_or_else(|| invalid("Edge separation needs exact line data"))?;
            let start = curve_point(curve, 0.)?;
            let end = curve_point(curve, 1.)?;
            let delta = Vector3::from(end) - Vector3::from(start);
            let length = delta.norm();
            // Positive rational weights put the complete curve in its control
            // hull; collinearity therefore proves the whole carrier is a line.
            if length < 1e-9
                || curve
                    .weights
                    .iter()
                    .any(|weight| !weight.is_finite() || *weight <= 0.)
                || curve.control_points.iter().any(|point| {
                    (Vector3::from(*point) - Vector3::from(start))
                        .cross(&delta)
                        .norm()
                        > length * length.max(1.) * 1e-9
                })
            {
                return Err(invalid(
                    "Edge separation requires noncollapsed exact straight edges",
                ));
            }
            Ok(LinearGeometry {
                start: transform(start, place.matrix),
                end: transform(end, place.matrix),
            })
        }
        CurveReference::SketchMember {
            sketch,
            member,
            instance_path,
        } => {
            let place = unique_placement(doc, e, sheet, &sketch.definition, instance_path)?;
            let geometry = e
                .sketches
                .get(&format!("{}::{}", place.key, sketch.node))
                .ok_or_else(|| invalid("The referenced sketch is missing or suppressed"))?;
            let solved = geometry
                .solved
                .as_ref()
                .ok_or_else(|| invalid("Exact solved sketch geometry is unavailable"))?;
            if crate::sketch_transform::suppressed_members(solved).contains(member) {
                return Err(invalid(
                    "The referenced straight sketch member is suppressed",
                ));
            }
            let source = solved
                .members
                .get(member)
                .ok_or_else(|| invalid("The referenced sketch member was removed"))?;
            let crate::SketchCurve::Line { start, end } = &source.curve else {
                return Err(invalid("Edge separation requires straight sketch members"));
            };
            let at = |id: &str| -> Result<[f64; 3]> {
                let point = geometry
                    .points
                    .get(id)
                    .ok_or_else(|| invalid("The referenced sketch endpoint was removed"))?;
                Ok(transform(
                    std::array::from_fn(|axis| {
                        geometry.plane.origin[axis]
                            + geometry.plane.u[axis] * point[0]
                            + geometry.plane.v[axis] * point[1]
                    }),
                    place.matrix,
                ))
            };
            Ok(LinearGeometry {
                start: at(start)?,
                end: at(end)?,
            })
        }
    }
}
/// Returns unit direction and origins in unscaled model-to-view coordinates.
pub fn parallel_lines(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    view: &str,
    first: &CurveReference,
    second: &CurveReference,
) -> Result<([f64; 2], [f64; 2], [f64; 2])> {
    if first == second {
        return Err(invalid("Select two different straight edges"));
    }
    let a = linear(doc, e, sheet, first)?;
    let b = linear(doc, e, sheet, second)?;
    let origin = project(view, a.start)?;
    let other = project(view, b.start)?;
    let end = project(view, a.end)?;
    let other_end = project(view, b.end)?;
    let delta = [end[0] - origin[0], end[1] - origin[1]];
    let peer = [other_end[0] - other[0], other_end[1] - other[1]];
    let length = delta[0].hypot(delta[1]);
    let peer_length = peer[0].hypot(peer[1]);
    if length < 1e-9
        || peer_length < 1e-9
        || (delta[0] * peer[1] - delta[1] * peer[0]).abs() > length * peer_length * 1e-8
    {
        return Err(invalid(
            "The selected edges must be parallel and noncollapsed in this drawing view",
        ));
    }
    Ok(([delta[0] / length, delta[1] / length], origin, other))
}
pub fn edge_separation_points(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    dimension: &DrawingDimension,
) -> Result<([f64; 2], [f64; 2])> {
    let (direction, first, second) = parallel_lines(
        doc,
        e,
        sheet,
        &dimension.view,
        dimension
            .options
            .curve
            .as_ref()
            .ok_or_else(|| invalid("Select the first straight edge"))?,
        dimension
            .options
            .second_curve
            .as_ref()
            .ok_or_else(|| invalid("Select the second straight edge"))?,
    )?;
    let picked = project(
        &dimension.view,
        point(doc, e, sheet, dimension.first, &dimension.options.first).unwrap_or(dimension.first),
    )?;
    let along = (picked[0] - first[0]) * direction[0] + (picked[1] - first[1]) * direction[1];
    let a = [
        first[0] + along * direction[0],
        first[1] + along * direction[1],
    ];
    let other_along = (a[0] - second[0]) * direction[0] + (a[1] - second[1]) * direction[1];
    Ok((
        a,
        [
            second[0] + other_along * direction[0],
            second[1] + other_along * direction[1],
        ],
    ))
}
pub fn circular(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    reference: &CurveReference,
) -> Result<CircularGeometry> {
    match reference {
        CurveReference::SketchMember {
            sketch,
            member,
            instance_path,
        } => {
            let place = unique_placement(doc, e, sheet, &sketch.definition, instance_path)?;
            let geometry = e
                .sketches
                .get(&format!("{}::{}", place.key, sketch.node))
                .ok_or_else(|| invalid("Circular sketch reference is missing or suppressed"))?;
            if geometry.solved.as_ref().is_some_and(|sketch| {
                crate::sketch_transform::suppressed_members(sketch).contains(member)
            }) {
                return Err(invalid(
                    "The referenced circular sketch member is suppressed",
                ));
            }
            let member_curve = geometry.solved.as_ref().and_then(|sketch| {
                sketch
                    .members
                    .get(member)
                    .map(|member| (&member.curve, sketch))
            });
            let (center, radius, start) = if let Some((curve, sketch)) = member_curve {
                match curve {
                crate::SketchCurve::Circle {center,radius}=>{
                    let c=sketch.points[center];let r=configured_value(doc,&reference_definition(reference),&place.key,radius,Dimension::Length)?;
                    (c,r,[c[0]+r,c[1]])
                },
                crate::SketchCurve::Arc {start,through,end}=>{
                    let (c,r,_,_)=crate::sketch_geometry::arc_frame(sketch.points[start],sketch.points[through],sketch.points[end])?;(c,r,sketch.points[start])
                },
                _=>return Err(CadError::Unsupported("Circular dimensions require a circle or circular arc; ellipse and spline dimensions need different annotation intent".into()))
            }
            } else if let Some(circle) = geometry.circles.get(member) {
                let c = *geometry
                    .points
                    .get(&circle.center)
                    .ok_or_else(|| invalid("Circular member center was removed"))?;
                (c, circle.radius_mm, [c[0] + circle.radius_mm, c[1]])
            } else {
                return Err(invalid(
                    "Referenced circular member was removed or suppressed",
                ));
            };
            let at = |p: [f64; 2]| {
                transform(
                    std::array::from_fn(|i| {
                        geometry.plane.origin[i]
                            + geometry.plane.u[i] * p[0]
                            + geometry.plane.v[i] * p[1]
                    }),
                    place.matrix,
                )
            };
            let local = Vector3::from(geometry.plane.u)
                .cross(&Vector3::from(geometry.plane.v))
                .normalize();
            let normal = std::array::from_fn(|i| {
                place.matrix[i][0] * local.x
                    + place.matrix[i][1] * local.y
                    + place.matrix[i][2] * local.z
            });
            Ok(CircularGeometry {
                center: at(center),
                normal,
                radius,
                start: at(start),
            })
        }
        CurveReference::ModelEdge {
            edge,
            body,
            instance_path,
        } => {
            let (edge, place) = resolve_edge(doc, e, sheet, edge, body, instance_path)?;
            let curve = edge.curve.as_ref().ok_or_else(|| {
                CadError::Unsupported(
                    "Circular model-edge dimensions require exact rational curve data".into(),
                )
            })?;
            circular_carrier(curve, place.matrix)
        }
    }
}
/// Recognize an exact carrier already obtained from the represented evaluation.
/// Candidate generation can use its known edge/placement directly; saved
/// references still go through `circular` and its unique provenance lookup.
pub(crate) fn circular_carrier(
    curve: &crate::sketch_edit::Curve3,
    matrix: [[f64; 4]; 4],
) -> Result<CircularGeometry> {
    let mut circle = conic_circle(curve)?;
    circle.center = transform(circle.center, matrix);
    circle.start = transform(circle.start, matrix);
    let n = Vector3::from(circle.normal);
    circle.normal =
        std::array::from_fn(|i| matrix[i][0] * n.x + matrix[i][1] * n.y + matrix[i][2] * n.z);
    Ok(circle)
}
fn reference_definition(reference: &CurveReference) -> String {
    match reference {
        CurveReference::SketchMember { sketch, .. } => sketch.definition.clone(),
        CurveReference::ModelEdge { edge, .. } => edge.producer.definition.clone(),
    }
}
fn configured_value(
    doc: &Document,
    definition: &str,
    key: &str,
    expression: &Expr,
    dimension: Dimension,
) -> Result<f64> {
    let Some(configuration) = key.strip_prefix(&format!("{definition}@")) else {
        return crate::expression::value(doc, definition, expression, dimension);
    };
    let configured = crate::configuration::variant(doc, definition, Some(configuration))?;
    crate::expression::value(&configured, definition, expression, dimension)
}
// A clamped rational quadratic span is circular only if its homogeneous
// polynomial satisfies the circle equation. Sampling alone does not establish it.
fn conic_circle(curve: &crate::sketch_edit::Curve3) -> Result<CircularGeometry> {
    if curve.degree != 2
        || curve.control_points.len() != 3
        || curve.knots.len() != 6
        || curve.knots[..3].iter().any(|v| *v != curve.knots[0])
        || curve.knots[3..].iter().any(|v| *v != curve.knots[3])
    {
        return Err(CadError::Unsupported("Circular model-edge recognition currently requires one clamped rational quadratic span".into()));
    }
    let a = Vector3::from(curve_point(curve, 0.)?);
    let b = Vector3::from(curve_point(curve, 0.5)?);
    let c = Vector3::from(curve_point(curve, 1.)?);
    let x = b - a;
    let y = c - a;
    let n = x.cross(&y);
    let nn = n.norm_squared();
    if !nn.is_finite() || nn < 1e-20 {
        return Err(invalid(
            "Selected edge is straight or has no resolvable circle",
        ));
    }
    let center = a + (y.cross(&n) * x.norm_squared() + n.cross(&x) * y.norm_squared()) / (2. * nn);
    let radius = (a - center).norm();
    let normal = n.normalize();
    if !radius.is_finite() || radius <= 0. {
        return Err(invalid("Circular edge has an invalid radius"));
    }
    let weighted = curve
        .control_points
        .iter()
        .zip(&curve.weights)
        .map(|(p, w)| (Vector3::from(*p) - center) * *w)
        .collect::<Vec<_>>();
    let q = [
        weighted[0],
        (weighted[1] - weighted[0]) * 2.,
        weighted[0] - weighted[1] * 2. + weighted[2],
    ];
    let w = [
        curve.weights[0],
        2. * (curve.weights[1] - curve.weights[0]),
        curve.weights[0] - 2. * curve.weights[1] + curve.weights[2],
    ];
    let bound =
        radius.max(1.).powi(2) * curve.weights.iter().copied().fold(1_f64, f64::max).powi(2) * 1e-8;
    for power in 0..=4 {
        let coefficient = (0..=2)
            .filter(|i| power >= *i && power - *i <= 2)
            .map(|i| q[i].dot(&q[power - i]) - radius * radius * w[i] * w[power - i])
            .sum::<f64>();
        if coefficient.abs() > bound {
            return Err(CadError::Unsupported("The selected rational conic is not circular; an ellipse cannot be annotated with a single radius".into()));
        }
    }
    if weighted
        .iter()
        .any(|p| p.dot(&normal).abs() > radius.max(1.) * 1e-8)
    {
        return Err(invalid("Circular edge controls are not planar"));
    }
    Ok(CircularGeometry {
        center: center.into(),
        normal: normal.into(),
        radius,
        start: a.into(),
    })
}

pub fn validate_definition(sheet: &Drawing) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    if sheet.id.is_empty()
        || sheet.name.trim().is_empty()
        || sheet.name.chars().any(char::is_control)
        || sheet
            .paper_mm
            .iter()
            .any(|v| !v.is_finite() || !(50. ..=10000.).contains(v))
        || sheet.annotations.iter().any(|s| !text_valid(s))
        || sheet
            .title_block
            .iter()
            .any(|(key, value)| !text_valid(key) || !text_valid(value))
    {
        return Err(invalid(
            "Sheets need an identity, a name, finite 50–10000 mm paper and valid note text",
        ));
    }
    ids.insert(sheet.id.clone());
    let views = view_keys(sheet);
    if sheet
        .view_display_modes
        .keys()
        .chain(sheet.view_visibility_geometry.keys())
        .any(|key| !views.contains(key))
    {
        return Err(invalid(
            "A drawing visibility mode refers to a view absent from its sheet",
        ));
    }
    if views.len() > 16
        || views.iter().collect::<BTreeSet<_>>().len() != views.len()
        || views.iter().any(|v| !VIEW_KEYS.contains(&v.as_str()))
    {
        return Err(invalid(
            "Sheet view keys must be distinct supported named projections",
        ));
    }
    if crate::drawing_visibility::mode_for(sheet, "section-z")
        != crate::drawing_visibility::DisplayMode::Wireframe
    {
        return Err(invalid(
            "Section-Z is a sampled section intersection; line visibility modes apply to full model views",
        ));
    }
    for key in &views {
        if !ids.insert(view_id(sheet, key)) {
            return Err(invalid("Drawing view semantic identities must be unique"));
        }
    }
    if sheet.dimensions.iter().any(|d| !views.contains(&d.view))
        || sheet.entities.iter().any(|a| !views.contains(&a.view))
    {
        return Err(invalid(
            "Move or delete view annotations before removing their view",
        ));
    }
    if let Some(format) = &sheet.format {
        validate_format(format, sheet.paper_mm)?;
        if let Some(block) = &format.title_block {
            for cell in &block.cells {
                if !ids.insert(cell.id.clone()) {
                    return Err(invalid(
                        "Title cells must have unique sheet-owned identities",
                    ));
                }
            }
        }
    }
    // Paper containment is evaluated against resolved views, title/table space
    // and inherited scales. Source remains editable when that layout is invalid.
    for (key, layout) in &sheet.view_layouts {
        let [_, _, width, height] = layout.rect_mm;
        if !views.contains(key)
            || layout.rect_mm.iter().any(|v| !v.is_finite())
            || width < 8.
            || height < 8.
            || layout.scale.is_some_and(|v| !v.is_finite() || v <= 0.)
        {
            return Err(invalid(
                "Saved view layouts need an existing view, a finite paper rectangle at least 8 mm wide and high and a positive optional scale",
            ));
        }
    }
    validate_view_relationships(sheet)?;
    let pages: BTreeSet<_> = sheet
        .tables
        .iter()
        .flat_map(|table| &table.segments)
        .map(|segment| segment.page)
        .filter(|page| *page > 0)
        .collect();
    if pages
        .iter()
        .copied()
        .ne(1..=pages.last().copied().unwrap_or(0))
    {
        return Err(invalid(
            "Continuation pages must be consecutive; repaginate or move sections to fill absent page offsets",
        ));
    }
    let mut datums = std::collections::BTreeSet::new();
    let mut hole_prefixes = BTreeSet::new();
    for table in &sheet.tables {
        if !ids.insert(table.id.clone()) {
            return Err(invalid(
                "Tables, annotations and dimensions need unique identities",
            ));
        }
        crate::drawing_tables::validate(table)?;
        for segment in &table.segments {
            if !ids.insert(segment.id.clone()) {
                return Err(invalid("Table segments need unique sheet-owned identities"));
            }
        }
        for row_id in crate::drawing_tables::owned_row_ids(table) {
            if !ids.insert(row_id) {
                return Err(invalid(
                    "Drawing table row identities must be unique within their sheet",
                ));
            }
        }
        if let crate::drawing_tables::TableKind::Holes { view, .. } = &table.kind
            && !views.contains(view)
        {
            return Err(invalid(
                "The hole table coordinate view must remain on its sheet",
            ));
        }
        if let crate::drawing_tables::TableKind::Holes {
            view, tag_prefix, ..
        } = &table.kind
            && !hole_prefixes.insert((view.clone(), tag_prefix.clone()))
        {
            return Err(invalid(
                "Hole tables on the same view require distinct tag prefixes",
            ));
        }
        if let crate::drawing_tables::TableKind::Revisions { rows } = &table.kind {
            for row in rows {
                if !ids.insert(row.id.clone()) {
                    return Err(invalid(
                        "Revision rows must have unique sheet-owned identities",
                    ));
                }
            }
        }
    }
    for d in &sheet.dimensions {
        if d.id.is_empty()
            || !ids.insert(d.id.clone())
            || d.options.precision > 8
            || d.first
                .iter()
                .chain(d.second.iter())
                .any(|p| !p.is_finite())
            || !d.offset_mm.is_finite()
            || d.options.third_point.iter().any(|p| !p.is_finite())
        {
            return Err(invalid(
                "Dimensions require unique identities, finite points and precision 0..8",
            ));
        }
    }
    let mut datum_target_labels = BTreeSet::new();
    for annotation in &sheet.entities {
        if let AnnotationKind::DatumTarget { symbol, .. } = &annotation.kind
            && !datum_target_labels.insert((symbol.identifier.to_uppercase(), symbol.number))
        {
            return Err(invalid(
                "Datum target letter/number labels must be unique on the sheet",
            ));
        }
        if annotation.id.is_empty()
            || !ids.insert(annotation.id.clone())
            || annotation.offset_mm.iter().any(|v| !v.is_finite())
            || annotation.leader_points_mm.len() > 64
            || annotation
                .leader_points_mm
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err(invalid(
                "Annotations require unique identities and finite label offsets",
            ));
        }
        if !annotation.leader_points_mm.is_empty()
            && !matches!(
                annotation.kind,
                AnnotationKind::Leader { .. }
                    | AnnotationKind::Datum { .. }
                    | AnnotationKind::GeometricTolerance { .. }
                    | AnnotationKind::DatumTarget { .. }
                    | AnnotationKind::GeometricToleranceFrame { .. }
                    | AnnotationKind::Balloon { .. }
                    | AnnotationKind::CutListBalloon { .. }
            )
        {
            return Err(invalid(
                "Elbow leaders are supported for notes, datums, geometric tolerances and balloons; this symbol has its own fixed routing",
            ));
        }
        match &annotation.kind {
            AnnotationKind::RevisionCloud { cloud, .. } => {
                cloud.validate()?;
                cloud.revision_label(sheet)?;
            }
            AnnotationKind::DatumTarget { symbol, .. } => symbol.validate()?,
            AnnotationKind::GeometricToleranceFrame { frame, .. } => frame.validate()?,
            AnnotationKind::DimensionGroup { group } => group.validate()?,
            AnnotationKind::SurfaceFinish { symbol, .. } => symbol.validate()?,
            AnnotationKind::WeldSymbol { symbol, .. } => symbol.validate()?,
            AnnotationKind::CutListBalloon {
                table,
                row_key,
                source,
                ..
            } => {
                let saved = sheet.tables.iter().find(|t| &t.id == table && matches!(t.kind, crate::drawing_tables::TableKind::WeldmentCutList { .. })).ok_or_else(|| invalid("A cut-list balloon must retain its cut-list table; reattach or remove balloons before deleting their table"))?;
                if row_key.is_empty()
                    || row_key.len() > 8192
                    || !saved.item_numbers.contains_key(row_key)
                    || source.occurrence_path.len() > 256
                    || source.output.definition != source.member.group.definition
                {
                    return Err(invalid(
                        "Cut-list balloons need a retained row reservation and a portable member/occurrence source",
                    ));
                }
            }
            AnnotationKind::Balloon { table, .. }
                if !sheet.tables.iter().any(|t| {
                    &t.id == table
                        && matches!(t.kind, crate::drawing_tables::TableKind::BillOfMaterials)
                }) =>
            {
                return Err(invalid(
                    "A balloon must retain a BOM table on the same sheet; reattach or remove its balloons before deleting the table",
                ));
            }
            AnnotationKind::Datum { identifier, .. }
                if identifier.is_empty()
                    || identifier.len() > 3
                    || !identifier.chars().all(|c| c.is_ascii_alphabetic()) =>
            {
                return Err(invalid("Datum identifier must contain 1–3 letters"));
            }
            AnnotationKind::CenterMark { extension_mm, .. }
            | AnnotationKind::CenterLine { extension_mm, .. }
                if !extension_mm.is_finite() || *extension_mm < 0. =>
            {
                return Err(invalid("Center extensions must be finite and nonnegative"));
            }
            AnnotationKind::GeometricTolerance { datums, .. }
                if datums.len() > 3
                    || datums
                        .iter()
                        .any(|d| d.is_empty() || !d.chars().all(|c| c.is_ascii_alphabetic())) =>
            {
                return Err(invalid(
                    "A geometric tolerance may reference up to three lettered datums",
                ));
            }
            _ => {}
        }
        if let AnnotationKind::Datum { identifier, .. } = &annotation.kind
            && !datums.insert(identifier)
        {
            return Err(invalid("Datum identifiers must be unique on a sheet"));
        }
        let anchors: Vec<&Anchor> = match &annotation.kind {
            AnnotationKind::DimensionGroup { group } => std::iter::once(&group.datum)
                .chain(group.targets.iter().map(|target| &target.anchor))
                .collect(),
            AnnotationKind::Leader { target, .. }
            | AnnotationKind::Datum { target, .. }
            | AnnotationKind::Balloon { target, .. }
            | AnnotationKind::CutListBalloon { target, .. }
            | AnnotationKind::SurfaceFinish { target, .. }
            | AnnotationKind::WeldSymbol { target, .. }
            | AnnotationKind::GeometricTolerance { target, .. }
            | AnnotationKind::DatumTarget { target, .. }
            | AnnotationKind::GeometricToleranceFrame { target, .. } => vec![target],
            AnnotationKind::RevisionCloud { target, .. } => vec![target],
            AnnotationKind::CenterLine { first, second, .. } => vec![first, second],
            AnnotationKind::CenterMark { .. } => vec![],
        };
        if anchors
            .iter()
            .any(|a| a.point.iter().any(|v| !v.is_finite()))
        {
            return Err(invalid(
                "Annotation targets must contain finite source coordinates",
            ));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct View {
    name: String,
    at: [f64; 2],
    size: [f64; 2],
    center: [f64; 2],
    scale: f64,
}
type PreparedView = (
    View,
    Vec<Vec<[f64; 3]>>,
    Vec<Option<crate::sketch_edit::Curve3>>,
    String,
    [f64; 2],
    [f64; 2],
    Vec<bool>,
);
fn validate_view_relationships(sheet: &Drawing) -> Result<()> {
    let keys = view_keys(sheet);
    for child in &keys {
        let mut seen = BTreeSet::new();
        let mut current = child.as_str();
        while let Some(alignment) = sheet
            .view_layouts
            .get(current)
            .and_then(|layout| layout.alignment.as_ref())
        {
            if !keys.contains(&alignment.parent)
                || !seen.insert(current.to_owned())
                || alignment.parent == current
            {
                return Err(invalid(
                    "View alignment needs a different existing parent and an acyclic relationship",
                ));
            }
            current = &alignment.parent;
        }
    }
    Ok(())
}
fn resolve_view_alignment(
    sheet: &Drawing,
    key: &str,
    prepared: &mut BTreeMap<String, PreparedView>,
    resolved: &mut BTreeSet<String>,
) -> Result<()> {
    if resolved.contains(key) {
        return Ok(());
    }
    if let Some(alignment) = sheet
        .view_layouts
        .get(key)
        .and_then(|layout| layout.alignment.as_ref())
    {
        resolve_view_alignment(sheet, &alignment.parent, prepared, resolved)?;
        let parent = prepared
            .get(&alignment.parent)
            .ok_or_else(|| invalid("Missing aligned parent view"))?
            .0
            .clone();
        let child = &mut prepared
            .get_mut(key)
            .ok_or_else(|| invalid("Missing aligned child view"))?
            .0;
        if alignment.shared_scale {
            child.scale = parent.scale;
        }
        let axis = match alignment.axis {
            ViewAlignmentAxis::Free => None,
            ViewAlignmentAxis::Horizontal => Some(1),
            ViewAlignmentAxis::Vertical => Some(0),
        };
        if let Some(axis) = axis {
            // Align the projected model origin, not the independently fitted bounding-box centers.
            child.at[axis] = parent.at[axis] + parent.size[axis] * 0.5
                - parent.center[axis] * parent.scale
                + child.center[axis] * child.scale
                - child.size[axis] * 0.5;
        }
    }
    resolved.insert(key.into());
    Ok(())
}
/// Familiar production ratios, choosing the largest that fits every linked
/// view. Unusual part sizes extend the same 1/2/5 sequence by powers of ten.
fn standard_scale(limit: f64) -> f64 {
    let power = 10f64.powf(limit.log10().floor());
    [1., 2., 5.]
        .into_iter()
        .map(|factor| factor * power)
        .filter(|scale| *scale <= limit * (1. + 1e-12))
        .fold(power, f64::max)
}
pub fn scale_label(scale: f64) -> String {
    if (scale - 1.).abs() < 1e-9 {
        return "1:1".into();
    }
    if scale > 1. {
        return format!("{}:1", trim_number(scale));
    }
    format!("1:{}", trim_number(1. / scale))
}
fn trim_number(number: f64) -> String {
    format!("{number:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into()
}
pub(crate) fn project(name: &str, p: [f64; 3]) -> Result<[f64; 2]> {
    Ok(match name {
        "front" | "detail-front" => [p[0], p[2]],
        "top" | "section-z" => [p[0], p[1]],
        "right" => [p[1], p[2]],
        "isometric" => [
            (p[0] - p[1]) * std::f64::consts::FRAC_1_SQRT_2,
            p[2] * (2f64 / 3.).sqrt() + (p[0] + p[1]) / 6f64.sqrt(),
        ],
        _ => return Err(CadError::Unsupported(format!("Drawing view {name}"))),
    })
}
impl View {
    fn screen(&self, p: [f64; 3]) -> Result<[f64; 2]> {
        let p = project(&self.name, p)?;
        Ok([
            self.at[0] + self.size[0] / 2. + (p[0] - self.center[0]) * self.scale,
            self.at[1] + self.size[1] / 2. + (p[1] - self.center[1]) * self.scale,
        ])
    }
}
fn emit_projected_curve(
    out: &mut Vec<Primitive>,
    view: &View,
    curve: crate::sketch_edit::Curve3,
    dashed: bool,
    hidden: bool,
    clip: bool,
) -> Result<()> {
    let curve = crate::sketch_edit::Curve2 {
        control_points: curve
            .control_points
            .into_iter()
            .map(|p| view.screen(p))
            .collect::<Result<_>>()?,
        weights: curve.weights,
        knots: curve.knots,
        degree: curve.degree,
    };
    crate::sketch_edit::validate_nurbs(&curve)?;
    if curve.control_points.iter().all(|p| {
        (p[0] - curve.control_points[0][0]).hypot(p[1] - curve.control_points[0][1]) < 1e-10
    }) {
        return Ok(());
    }
    if clip {
        for pair in display_curve(&curve)?.windows(2) {
            if let Some([a, b]) = clipped_segment(pair[0], pair[1], view) {
                out.push(if hidden {
                    Primitive::HiddenPolyline { points: vec![a, b] }
                } else {
                    Primitive::Polyline {
                        points: vec![a, b],
                        dashed,
                    }
                });
            }
        }
    } else {
        out.push(if hidden {
            Primitive::HiddenSpline { curve }
        } else {
            Primitive::Spline { curve, dashed }
        });
    }
    Ok(())
}
struct TraceView<'a> {
    view: &'a View,
    mode: crate::drawing_visibility::DisplayMode,
    clip: bool,
    tolerance_mm: f64,
}
impl TraceView<'_> {
    fn emit(
        &self,
        out: &mut Vec<Primitive>,
        bindings: &mut Vec<NativeTraceBinding>,
        storage_units: &mut usize,
        trace: crate::drawing_visibility::NativeTrace,
    ) -> Result<()> {
        if !trace.max_error_mm.is_finite()
            || trace.max_error_mm < 0.
            || trace.max_error_mm > self.tolerance_mm
            || trace.points.len() < 2
            || trace.points.iter().flatten().any(|p| !p.is_finite())
        {
            return Err(invalid(
                "Native drawing trace has no valid curve-output error certificate",
            ));
        }
        // Bound the entire sheet, not each view independently. Include the
        // paper copy of each point and binding/primitive allocation overhead.
        *storage_units = storage_units
            .saturating_add(trace.storage_units())
            .saturating_add(trace.points.len().saturating_mul(4))
            .saturating_add(64);
        if *storage_units > 8_000_000 {
            return Err(invalid(
                "Native drawing output exceeds the bounded sheet storage budget; reduce the displayed scope or split the views across sheets",
            ));
        }
        let start = out.len();
        if !trace.collapsed_projection
            && (!trace.hidden
                || self.mode != crate::drawing_visibility::DisplayMode::HiddenLinesRemoved)
        {
            let points = trace
                .points
                .iter()
                .map(|p| self.view.screen(*p))
                .collect::<Result<Vec<_>>>()?;
            let mut append = |points| {
                out.push(if trace.hidden {
                    Primitive::HiddenPolyline { points }
                } else {
                    Primitive::Polyline {
                        points,
                        dashed: trace.ghost,
                    }
                })
            };
            if self.clip {
                for pair in points.windows(2) {
                    if let Some([a, b]) = clipped_segment(pair[0], pair[1], self.view) {
                        append(vec![a, b]);
                    }
                }
            } else {
                append(points);
            }
        }
        bindings.push(NativeTraceBinding {
            view: self.view.name.clone(),
            primitive_range: [start, out.len()],
            geometry: trace,
        });
        Ok(())
    }
}
/// Display/print tessellation only. DXF retains the exact projected rational curve.
pub fn display_curve(curve: &crate::sketch_edit::Curve2) -> Result<Vec<[f64; 2]>> {
    crate::sketch_edit::validate_nurbs(curve)?;
    let start = curve.knots[curve.degree];
    let end = curve.knots[curve.control_points.len()];
    let mut points = vec![];
    for span in curve
        .knots
        .windows(2)
        .filter(|span| span[1] > span[0] && span[0] >= start && span[1] <= end)
    {
        for index in 0..32 {
            let t = (span[0] + (span[1] - span[0]) * index as f64 / 32. - start) / (end - start);
            points.push(crate::sketch_edit::nurbs_point(curve, t)?);
        }
    }
    points.push(crate::sketch_edit::nurbs_point(curve, 1.)?);
    Ok(points)
}
fn clipped_segment(a: [f64; 2], b: [f64; 2], view: &View) -> Option<[[f64; 2]; 2]> {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let mut enter = 0_f64;
    let mut exit = 1_f64;
    for (p, q) in [
        (-delta[0], a[0] - view.at[0]),
        (delta[0], view.at[0] + view.size[0] - a[0]),
        (-delta[1], a[1] - view.at[1]),
        (delta[1], view.at[1] + view.size[1] - a[1]),
    ] {
        if p.abs() < 1e-12 {
            if q < 0. {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0. {
                enter = enter.max(t);
            } else {
                exit = exit.min(t);
            }
            if enter > exit {
                return None;
            }
        }
    }
    Some([
        [a[0] + delta[0] * enter, a[1] + delta[1] * enter],
        [a[0] + delta[0] * exit, a[1] + delta[1] * exit],
    ])
}
fn line(out: &mut Vec<Primitive>, a: [f64; 2], b: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![a, b],
        dashed: false,
    });
}
fn text(out: &mut Vec<Primitive>, at: [f64; 2], size: f64, value: impl ToString) {
    out.push(Primitive::Text {
        at,
        size,
        text: value.to_string(),
    });
}
fn rectangle(out: &mut Vec<Primitive>, at: [f64; 2], size: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![
            at,
            [at[0] + size[0], at[1]],
            [at[0] + size[0], at[1] + size[1]],
            [at[0], at[1] + size[1]],
            at,
        ],
        dashed: false,
    });
}
fn wrapped_title(value: &str, width: f64, size: f64) -> Vec<String> {
    // One text-size width per character conservatively fits the default vector
    // font without clipping or silently truncating the linked property.
    let capacity = ((width - 2. * TITLE_CELL_PADDING_MM) / size)
        .floor()
        .max(1.) as usize;
    let mut rows = vec![];
    for line in value.split('\n') {
        let characters = line.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            rows.push(String::new());
        } else {
            let mut remaining = characters.as_slice();
            let mut word_rows = vec![];
            while remaining.len() > capacity {
                let space = remaining[..=capacity]
                    .iter()
                    .rposition(|c| *c == ' ')
                    .filter(|index| *index > 0);
                let end = space.unwrap_or(capacity);
                word_rows.push(remaining[..end].iter().collect::<String>());
                remaining = &remaining[end + usize::from(space.is_some())..];
            }
            if !remaining.is_empty() {
                word_rows.push(remaining.iter().collect::<String>());
            }
            // Prefer whole words, but keep the existing hard-wrap fit for
            // explicit/custom cells when word boundaries would add rows.
            if word_rows.len() <= characters.len().div_ceil(capacity) {
                rows.extend(word_rows);
            } else {
                rows.extend(
                    characters
                        .chunks(capacity)
                        .map(|c| c.iter().collect::<String>()),
                );
            }
        }
    }
    rows
}
pub fn format_geometry(doc: &Document, sheet: &Drawing) -> Result<Vec<Primitive>> {
    let Some(format) = &sheet.format else {
        return Ok(vec![]);
    };
    validate_format(format, sheet.paper_mm)?;
    let mut out = vec![];
    let inner = paper_bounds(sheet);
    let outer = [
        [format.margins_mm[0], format.margins_mm[1]],
        [
            sheet.paper_mm[0] - format.margins_mm[2],
            sheet.paper_mm[1] - format.margins_mm[3],
        ],
    ];
    if format.border {
        rectangle(
            &mut out,
            outer[0],
            [outer[1][0] - outer[0][0], outer[1][1] - outer[0][1]],
        );
    }
    if let Some(zones) = &format.zones {
        rectangle(
            &mut out,
            inner[0],
            [inner[1][0] - inner[0][0], inner[1][1] - inner[0][1]],
        );
        let dx = (inner[1][0] - inner[0][0]) / zones.columns as f64;
        let dy = (inner[1][1] - inner[0][1]) / zones.rows as f64;
        for index in 0..=zones.columns {
            let x = inner[0][0] + index as f64 * dx;
            line(&mut out, [x, outer[0][1]], [x, inner[0][1]]);
            line(&mut out, [x, inner[1][1]], [x, outer[1][1]]);
            if index < zones.columns {
                let label = (index + 1).to_string();
                let center = x + dx / 2. - label.len() as f64;
                text(
                    &mut out,
                    [center, outer[0][1] + zones.band_mm / 2. - 1.],
                    2.,
                    &label,
                );
                text(
                    &mut out,
                    [center, inner[1][1] + zones.band_mm / 2. - 1.],
                    2.,
                    label,
                );
            }
        }
        for index in 0..=zones.rows {
            let y = inner[0][1] + index as f64 * dy;
            line(&mut out, [outer[0][0], y], [inner[0][0], y]);
            line(&mut out, [inner[1][0], y], [outer[1][0], y]);
            if index < zones.rows {
                let label = char::from(b'A' + index as u8).to_string();
                text(
                    &mut out,
                    [outer[0][0] + zones.band_mm / 2. - 1., y + dy / 2. - 1.],
                    2.,
                    &label,
                );
                text(
                    &mut out,
                    [inner[1][0] + zones.band_mm / 2. - 1., y + dy / 2. - 1.],
                    2.,
                    label,
                );
            }
        }
    }
    if let Some(block) = &format.title_block {
        let at = [inner[1][0] - block.size_mm[0], inner[0][1]];
        rectangle(&mut out, at, block.size_mm);
        for cell in &block.cells {
            let [x, y, width, height] = cell.rect_mm;
            let position = [at[0] + x, at[1] + y];
            rectangle(&mut out, position, [width, height]);
            let label_size = block.text_size_mm * TITLE_LABEL_SCALE;
            let label = wrapped_title(&cell.label, width, label_size);
            let value = wrapped_title(
                &linked_text(doc, sheet, &cell.value)?,
                width,
                block.text_size_mm,
            );
            let used = label.len() as f64 * label_size * TITLE_LINE_HEIGHT
                + value.len() as f64 * block.text_size_mm * TITLE_LINE_HEIGHT
                + 2. * TITLE_CELL_PADDING_MM;
            if used > height {
                return Err(invalid(format!(
                    "Title field '{}' exceeds its cell; enlarge it or reduce the text size",
                    cell.label
                )));
            }
            let mut baseline = position[1] + height - TITLE_CELL_PADDING_MM;
            for row in label {
                baseline -= label_size * TITLE_LINE_HEIGHT;
                text(
                    &mut out,
                    [position[0] + TITLE_CELL_PADDING_MM, baseline],
                    label_size,
                    row,
                );
            }
            for row in value {
                baseline -= block.text_size_mm * TITLE_LINE_HEIGHT;
                text(
                    &mut out,
                    [position[0] + TITLE_CELL_PADDING_MM, baseline],
                    block.text_size_mm,
                    row,
                );
            }
        }
    }
    Ok(out)
}
fn arrow(out: &mut Vec<Primitive>, tip: [f64; 2], toward: [f64; 2]) {
    let d = [toward[0] - tip[0], toward[1] - tip[1]];
    let size = d[0].hypot(d[1]);
    if size < 1e-9 {
        return;
    }
    let u = [d[0] / size, d[1] / size];
    for sign in [-1., 1.] {
        line(
            out,
            tip,
            [
                tip[0] + u[0] * 2.5 - u[1] * sign * 0.8,
                tip[1] + u[1] * 2.5 + u[0] * sign * 0.8,
            ],
        );
    }
}
fn owner_parameter(doc: &Document, d: &DrawingDimension) -> Result<Option<(f64, Dimension)>> {
    let Some(reference) = &d.parameter else {
        return Ok(None);
    };
    let p = doc
        .parts
        .get(&reference.definition)
        .map(|p| &p.parameters)
        .or_else(|| {
            doc.assemblies
                .get(&reference.definition)
                .map(|a| &a.parameters)
        })
        .and_then(|p| p.get(&reference.parameter))
        .ok_or_else(|| invalid("Dimension parameter reference is broken"))?;
    Ok(Some((
        crate::expression::value(doc, &reference.definition, &p.expression, p.dimension)?,
        p.dimension,
    )))
}
pub(crate) fn label(
    doc: &Document,
    sheet: &Drawing,
    d: &DrawingDimension,
    value: f64,
    angular: bool,
) -> Result<String> {
    let dimension = if angular {
        Dimension::Angle
    } else {
        Dimension::Length
    };
    let conversion = if angular {
        180. / std::f64::consts::PI
    } else if d.options.units == Units::Inches {
        1. / 25.4
    } else {
        1.
    };
    let format = |v: f64| format!("{:.*}", d.options.precision, v * conversion);
    let quantity = |e: &Expr| crate::expression::value(doc, &sheet.definition, e, dimension);
    let mut value = match &d.options.tolerance {
        Tolerance::None | Tolerance::Basic => format(value),
        Tolerance::Symmetric { value: t } => {
            let t = quantity(t)?;
            if t < 0. {
                return Err(invalid("Symmetric tolerance must be nonnegative"));
            }
            format!("{} ±{}", format(value), format(t))
        }
        Tolerance::Deviation { lower, upper } => {
            let lo = quantity(lower)?;
            let hi = quantity(upper)?;
            if lo > 0. || hi < 0. || lo > hi {
                return Err(invalid(
                    "Deviation tolerance needs a nonpositive lower and nonnegative upper deviation",
                ));
            }
            format!("{} +{} / {}", format(value), format(hi), format(lo))
        }
        Tolerance::Limits { lower, upper } => {
            let lo = quantity(lower)?;
            let hi = quantity(upper)?;
            if lo > hi {
                return Err(invalid("Lower dimension limit exceeds upper limit"));
            }
            format!("{} / {}", format(hi), format(lo))
        }
    };
    if angular {
        value.push('°');
    } else if d.options.units == Units::Inches {
        value.push_str(" in");
    }
    if !d.tolerance.is_empty() {
        value.push(' ');
        value.push_str(&d.tolerance);
    }
    if d.options.reference {
        value = format!("({value})");
    }
    Ok(format!("{}{}{}", d.options.prefix, value, d.options.suffix))
}

fn hole_callout(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    d: &DrawingDimension,
    curve: &CurveReference,
    circle: &CircularGeometry,
) -> Result<(Vec<String>, Option<CircularGeometry>)> {
    let feature = d
        .options
        .hole_feature
        .as_ref()
        .ok_or_else(|| invalid("Hole callout needs a designed hole feature"))?;
    let CurveReference::ModelEdge {
        edge,
        instance_path,
        ..
    } = curve
    else {
        return Err(CadError::Unsupported("An associative hole callout requires an exact circular model edge produced by its designed hole feature".into()));
    };
    if edge.producer != *feature {
        return Err(invalid(
            "The selected circular edge was not produced by the referenced hole feature",
        ));
    }
    let placement = unique_placement(doc, e, sheet, &feature.definition, instance_path)?;
    let configuration = placement
        .key
        .strip_prefix(&format!("{}@", feature.definition));
    let configured = crate::configuration::variant(doc, &feature.definition, configuration)?;
    let source = configured
        .parts
        .get(&feature.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|source| source.id == feature.node && !source.suppressed)
        })
        .ok_or_else(|| invalid("Hole feature is missing or suppressed in this configuration"))?;
    let length = |expression: &Expr| {
        crate::expression::value(
            &configured,
            &feature.definition,
            expression,
            Dimension::Length,
        )
    };
    let units = if d.options.units == Units::Inches {
        1. / 25.4
    } else {
        1.
    };
    let unit = if d.options.units == Units::Inches {
        " in"
    } else {
        " mm"
    };
    let size = |value: f64| format!("{:.*}{unit}", d.options.precision, value * units);
    match &source.operation {
        crate::Operation::Hole {radius,depth,..} => {
            let radius=length(radius)?;
            if (radius-circle.radius).abs()>1e-6 {return Err(invalid("The selected circle does not match the referenced Hole feature radius"));}
            Ok((vec![format!("Ø{} DEPTH {}",label(doc,sheet,d,radius*2.,false)?,size(length(depth)?))],None))
        }
        crate::Operation::DrilledHoles {definition} => {
            let diameter=length(definition.bore_diameter())?;
            let head_diameter=match &definition.head {crate::holes::Head::Simple=>None,crate::holes::Head::Counterbore {diameter,..}|crate::holes::Head::Countersink {diameter,..}=>Some(length(diameter)?)};
            if (diameter*0.5-circle.radius).abs()>1e-6 && head_diameter.is_none_or(|diameter|(diameter*0.5-circle.radius).abs()>1e-6) {return Err(invalid("The selected circle matches neither the hole diameter nor its designed head diameter"));}
            if definition.points.is_empty() || definition.points.iter().collect::<std::collections::BTreeSet<_>>().len()!=definition.points.len() {return Err(invalid("Designed hole positions are empty or contain duplicates"));}
            let position_key=if definition.positions.definition==feature.definition {placement.key.clone()}else{definition.positions.definition.clone()};
            let positions=e.sketches.get(&format!("{}::{}",position_key,definition.positions.node)).ok_or_else(||invalid("Configured hole-position sketch is missing or suppressed"))?;
            let normal=Vector3::from(positions.plane.u).cross(&Vector3::from(positions.plane.v));
            let normal=Vector3::new(placement.matrix[0][0]*normal.x+placement.matrix[0][1]*normal.y+placement.matrix[0][2]*normal.z,placement.matrix[1][0]*normal.x+placement.matrix[1][1]*normal.y+placement.matrix[1][2]*normal.z,placement.matrix[2][0]*normal.x+placement.matrix[2][1]*normal.y+placement.matrix[2][2]*normal.z);
            if normal.norm()<1e-9 || Vector3::from(circle.normal).cross(&normal.normalize()).norm()>1e-6 {return Err(invalid("The selected circular edge is not normal to the designed hole axis"));}
            let normal=normal.normalize();
            let mut matched_center=None;
            for point in &definition.points {
                let point=positions.points.get(point).ok_or_else(||invalid("A designed hole-center point was removed"))?;
                let center=transform(std::array::from_fn(|i|positions.plane.origin[i]+positions.plane.u[i]*point[0]+positions.plane.v[i]*point[1]),placement.matrix);
                if (Vector3::from(circle.center)-Vector3::from(center)).cross(&normal).norm()<1e-6 {matched_center=Some(center);}
            }
            let matched_center=matched_center.ok_or_else(||invalid("The selected circular edge is not on an authored hole-center axis"))?;
            let count=if definition.points.len()>1 {format!("{}X ",definition.points.len())}else{String::new()};
            let end=match definition.end {crate::holes::End::Blind=>format!("DEPTH {}",size(length(&definition.depth)?)),crate::holes::End::ThroughAll=>"THRU".into()};
            let thread=definition.thread.as_ref().map(|thread|thread.evaluate(&configured,&feature.definition)).transpose()?;
            let mut mark=None;
            let mut lines=if let Some(thread)=&thread {
                if thread.end==crate::holes::End::ThroughAll && definition.end!=crate::holes::End::ThroughAll {return Err(invalid("Through thread intent requires a Through All tap-drill bore"));}
                let head_depth=match &definition.head {
                    crate::holes::Head::Simple=>0.,
                    crate::holes::Head::Counterbore {depth,..}=>length(depth)?,
                    crate::holes::Head::Countersink {diameter:head_diameter,angle}=> {
                        let angle=crate::expression::value(&configured,&feature.definition,angle,Dimension::Angle)?;
                        (length(head_diameter)?-diameter)*0.5/(angle*0.5).tan()
                    },
                };
                if !head_depth.is_finite() || head_depth<0. {return Err(invalid("Cosmetic thread entry head depth is invalid"));}
                if definition.end==crate::holes::End::Blind {thread.validate_bore(definition.end,length(&definition.depth)?,head_depth)?;}
                let direction=if definition.reverse {normal} else {-normal};
                let center=Vector3::from(matched_center)+direction*(length(&definition.start_offset)?+head_depth);
                let radial=(Vector3::from(circle.start)-Vector3::from(circle.center)).normalize();
                mark=Some(CircularGeometry {center:center.into(),normal:circle.normal,radius:thread.nominal_diameter_mm*0.5,start:(center+radial*thread.nominal_diameter_mm*0.5).into()});
                let thread_end=match thread.depth_mm {Some(depth)=>format!("DEPTH {} BELOW ENTRY HEAD",size(depth)),None=>"THRU".into()};
                vec![
                    format!("{count}THREAD Ø{} PITCH {} {} {thread_end}",label(doc,sheet,d,thread.nominal_diameter_mm,false)?,size(thread.pitch_mm),thread.handedness.abbreviation()),
                    format!("TAP DRILL Ø{} {end}",size(thread.tap_drill_diameter_mm)),
                    "COSMETIC THREAD".into(),
                ]
            } else {vec![format!("{count}Ø{} {end}",label(doc,sheet,d,diameter,false)?) ]};
            match &definition.head {
                crate::holes::Head::Simple=>{},
                crate::holes::Head::Counterbore {diameter,depth}=>lines.push(format!("CBORE Ø{} DEPTH {}",size(length(diameter)?),size(length(depth)?))),
                crate::holes::Head::Countersink {diameter,angle}=>{let angle=crate::expression::value(&configured,&feature.definition,angle,Dimension::Angle)?;lines.push(format!("CSINK Ø{} x {:.*}°",size(length(diameter)?),d.options.precision,angle.to_degrees()));}
            }
            Ok((lines,mark))
        }
        _=>Err(CadError::Unsupported("Hole callouts require a saved Hole or DrilledHoles definition; thread intent is read only from an explicit cosmetic-thread definition".into())),
    }
}

/// Exact projected annotation geometry; this does not add a model edge or face.
fn cosmetic_thread_mark(view: &View, circle: &CircularGeometry) -> Result<Primitive> {
    let center = Vector3::from(circle.center);
    let normal = Vector3::from(circle.normal);
    let radial = Vector3::from(circle.start) - center;
    if !circle.radius.is_finite()
        || circle.radius <= 1e-7
        || !normal.norm().is_finite()
        || !radial.norm().is_finite()
        || normal.norm() < 1e-9
        || radial.norm() < 1e-9
    {
        return Err(invalid(
            "Cosmetic thread mark needs a finite nominal diameter and exact circular rim frame",
        ));
    }
    let radial = radial.normalize();
    let tangent = normal.normalize().cross(&radial).normalize();
    let radius = circle.radius;
    let control_points = [
        [1., 0.],
        [1., 1.],
        [0., 1.],
        [-1., 1.],
        [-1., 0.],
        [-1., -1.],
        [0., -1.],
    ]
    .into_iter()
    .map(|p| view.screen((center + (radial * p[0] + tangent * p[1]) * radius).into()))
    .collect::<Result<Vec<_>>>()?;
    let curve = crate::sketch_edit::Curve2 {
        control_points,
        weights: vec![
            1.,
            std::f64::consts::FRAC_1_SQRT_2,
            1.,
            std::f64::consts::FRAC_1_SQRT_2,
            1.,
            std::f64::consts::FRAC_1_SQRT_2,
            1.,
        ],
        knots: vec![0., 0., 0., 1. / 3., 1. / 3., 2. / 3., 2. / 3., 1., 1., 1.],
        degree: 2,
    };
    crate::sketch_edit::validate_nurbs(&curve)?;
    Ok(Primitive::Spline {
        curve,
        dashed: false,
    })
}

pub(crate) fn dimension_for_placement(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    projection: &ViewProjection,
    source: &DrawingDimension,
    out: &mut Vec<Primitive>,
) -> Result<()> {
    dimension(
        doc,
        evaluation,
        sheet,
        &View {
            name: source.view.clone(),
            at: [projection.rect_mm[0], projection.rect_mm[1]],
            size: [projection.rect_mm[2], projection.rect_mm[3]],
            center: projection.model_center,
            scale: projection.scale,
        },
        source,
        out,
    )
}

fn dimension(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    view: &View,
    d: &DrawingDimension,
    out: &mut Vec<Primitive>,
) -> Result<()> {
    let circular_dimension = matches!(
        d.options.kind,
        DimensionKind::Diameter | DimensionKind::Radius | DimensionKind::HoleCallout
    );
    let first = if circular_dimension {
        d.first
    } else if d.options.second_curve.is_some() || d.options.kind == DimensionKind::Perpendicular {
        // Curve carriers define the measurement. Saved point references only
        // place its extension along the carrier and may be repaired separately.
        point(doc, e, sheet, d.first, &d.options.first).unwrap_or(d.first)
    } else {
        point(doc, e, sheet, d.first, &d.options.first)?
    };
    let second = if circular_dimension
        || d.options.second_curve.is_some()
        || d.options.kind == DimensionKind::Perpendicular
    {
        d.second
    } else {
        point(doc, e, sheet, d.second, &d.options.second)?
    };
    let mut a = view.screen(first)?;
    let mut b = view.screen(second)?;
    let mut value = (Vector3::from(second) - Vector3::from(first)).norm();
    let mut prefix = "";
    if d.options.second_curve.is_some() || d.options.kind == DimensionKind::Perpendicular {
        if !matches!(
            d.options.kind,
            DimensionKind::Horizontal | DimensionKind::Vertical | DimensionKind::Perpendicular
        ) {
            return Err(invalid(
                "Two line carriers require an edge-separation dimension type",
            ));
        }
        let (direction, carrier, peer) = parallel_lines(
            doc,
            e,
            sheet,
            &view.name,
            d.options
                .curve
                .as_ref()
                .ok_or_else(|| invalid("Select the first straight edge"))?,
            d.options
                .second_curve
                .as_ref()
                .ok_or_else(|| invalid("Select the second straight edge"))?,
        )?;
        if (d.options.kind == DimensionKind::Horizontal && direction[0].abs() > 1e-8)
            || (d.options.kind == DimensionKind::Vertical && direction[1].abs() > 1e-8)
        {
            return Err(invalid(
                "The edge carriers no longer support this axis separation; use Perpendicular or reattach the dimension",
            ));
        }
        let picked = project(&view.name, first)?;
        let first_along =
            (picked[0] - carrier[0]) * direction[0] + (picked[1] - carrier[1]) * direction[1];
        let origin = [
            carrier[0] + first_along * direction[0],
            carrier[1] + first_along * direction[1],
        ];
        a = [
            a[0] + (origin[0] - picked[0]) * view.scale,
            a[1] + (origin[1] - picked[1]) * view.scale,
        ];
        let along = (origin[0] - peer[0]) * direction[0] + (origin[1] - peer[1]) * direction[1];
        let closest = [
            peer[0] + along * direction[0],
            peer[1] + along * direction[1],
        ];
        b = [
            a[0] + (closest[0] - origin[0]) * view.scale,
            a[1] + (closest[1] - origin[1]) * view.scale,
        ];
        value = (closest[0] - origin[0]).hypot(closest[1] - origin[1]);
    } else if matches!(
        d.options.kind,
        DimensionKind::Diameter | DimensionKind::Radius | DimensionKind::HoleCallout
    ) {
        let curve = d
            .options
            .curve
            .as_ref()
            .ok_or_else(|| invalid("Select a circular member or exact circular model edge"))?;
        let circle = circular(doc, e, sheet, curve)?;
        let center = view.screen(circle.center)?;
        let start = view.screen(circle.start)?;
        let projected_radius = (start[0] - center[0]).hypot(start[1] - center[1]);
        if projected_radius < 1e-9 {
            return Err(invalid(
                "The circular dimension collapses in this view; select another view",
            ));
        }
        b = start;
        if d.options.kind == DimensionKind::Radius {
            a = center;
            value = circle.radius;
            prefix = "R";
        } else {
            a = [center[0] * 2. - start[0], center[1] * 2. - start[1]];
            value = circle.radius * 2.;
            prefix = "Ø";
        }
        if d.options.kind == DimensionKind::HoleCallout {
            let (lines, mark) = hole_callout(doc, e, sheet, d, curve, &circle)?;
            if let Some(mark) = mark {
                out.push(cosmetic_thread_mark(view, &mark)?);
            }
            let at = [b[0] + d.offset_mm, b[1] + d.offset_mm];
            line(out, b, at);
            arrow(out, b, at);
            for (index, callout) in lines.into_iter().enumerate() {
                text(out, [at[0], at[1] - index as f64 * 4.5], 3., callout);
            }
            return Ok(());
        }
    } else if d.options.kind == DimensionKind::Angular {
        let third = point(doc, e, sheet, d.options.third_point, &d.options.third)?;
        let u = Vector3::from(second) - Vector3::from(first);
        let v = Vector3::from(third) - Vector3::from(first);
        if u.norm() < 1e-9 || v.norm() < 1e-9 {
            return Err(invalid("Angular dimension requires two nonzero rays"));
        }
        let projected_u = project(&view.name, u.into())?;
        let projected_v = project(&view.name, v.into())?;
        if (projected_u[0].powi(2) + projected_u[1].powi(2) - u.norm_squared()).abs()
            > u.norm_squared() * 1e-8
            || (projected_v[0].powi(2) + projected_v[1].powi(2) - v.norm_squared()).abs()
                > v.norm_squared() * 1e-8
        {
            return Err(CadError::Unsupported(
                "Angular dimension rays must lie parallel to the selected drawing view".into(),
            ));
        }
        value = (u.normalize().dot(&v.normalize())).clamp(-1., 1.).acos();
        let c = view.screen(first)?;
        let p = view.screen(second)?;
        let q = view.screen(third)?;
        let start = (p[1] - c[1]).atan2(p[0] - c[0]);
        let end = (q[1] - c[1]).atan2(q[0] - c[0]);
        let mut sweep = (end - start).rem_euclid(std::f64::consts::TAU);
        if sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        let radius = d.offset_mm.abs().max(3.);
        let arc = (0..=40)
            .map(|i| {
                let angle = start + sweep * i as f64 / 40.;
                [c[0] + radius * angle.cos(), c[1] + radius * angle.sin()]
            })
            .collect::<Vec<_>>();
        line(out, c, arc[0]);
        line(out, c, *arc.last().unwrap());
        arrow(out, arc[0], arc[1]);
        arrow(out, *arc.last().unwrap(), arc[arc.len() - 2]);
        out.push(Primitive::Polyline {
            points: arc,
            dashed: false,
        });
        if let Some((parameter, dim)) = owner_parameter(doc, d)? {
            if dim != Dimension::Angle {
                return Err(invalid("Angular dimension parameter must have angle units"));
            }
            value = parameter;
        }
        let angle = start + sweep / 2.;
        let at = [
            c[0] + (radius + 2.) * angle.cos(),
            c[1] + (radius + 2.) * angle.sin(),
        ];
        let label = label(doc, sheet, d, value, true)?;
        if d.options.tolerance == Tolerance::Basic {
            rectangle(
                out,
                [at[0] - 0.5, at[1] - 0.7],
                [label.chars().count() as f64 * 1.9 + 1., 4.],
            );
        }
        text(out, at, 3., label);
        return Ok(());
    }
    if let Some((parameter, dim)) = owner_parameter(doc, d)? {
        if dim != Dimension::Length {
            return Err(invalid(
                "Linear/circular dimension parameter must have length units",
            ));
        }
        value = parameter;
    }
    let (d1, d2) = match d.options.kind {
        DimensionKind::Horizontal => {
            if d.options.second_curve.is_none() {
                value = owner_parameter(doc, d)?.map(|(v, _)| v).unwrap_or(
                    (project(&view.name, second)?[0] - project(&view.name, first)?[0]).abs(),
                );
            }
            let y = a[1].max(b[1]) + d.offset_mm;
            ([a[0], y], [b[0], y])
        }
        DimensionKind::Vertical => {
            if d.options.second_curve.is_none() {
                value = owner_parameter(doc, d)?.map(|(v, _)| v).unwrap_or(
                    (project(&view.name, second)?[1] - project(&view.name, first)?[1]).abs(),
                );
            }
            let x = a[0].max(b[0]) + d.offset_mm;
            ([x, a[1]], [x, b[1]])
        }
        _ => {
            let delta = [b[0] - a[0], b[1] - a[1]];
            let size = delta[0].hypot(delta[1]);
            if size < 1e-9 {
                return Err(invalid("Dimension endpoints coincide in this view"));
            }
            let n = [-delta[1] / size, delta[0] / size];
            (
                [a[0] + n[0] * d.offset_mm, a[1] + n[1] * d.offset_mm],
                [b[0] + n[0] * d.offset_mm, b[1] + n[1] * d.offset_mm],
            )
        }
    };
    if (d1[0] - d2[0]).hypot(d1[1] - d2[1]) < 1e-9 {
        return Err(invalid("The chosen dimension direction has zero length"));
    }
    line(out, a, d1);
    line(out, b, d2);
    line(out, d1, d2);
    arrow(out, d1, d2);
    arrow(out, d2, d1);
    let at = [(d1[0] + d2[0]) / 2. + 1., (d1[1] + d2[1]) / 2. + 2.];
    let label = format!("{prefix}{}", label(doc, sheet, d, value, false)?);
    if d.options.tolerance == Tolerance::Basic {
        rectangle(
            out,
            [at[0] - 0.5, at[1] - 0.7],
            [label.chars().count() as f64 * 1.9 + 1., 4.],
        );
    }
    text(out, at, 3., label);
    Ok(())
}

fn anchor(doc: &Document, e: &Evaluation, sheet: &Drawing, a: &Anchor) -> Result<[f64; 3]> {
    point(doc, e, sheet, a.point, &a.reference)
}
pub(crate) fn characteristic(
    out: &mut Vec<Primitive>,
    at: [f64; 2],
    kind: &GeometricCharacteristic,
) {
    let c = [at[0] + 3., at[1] + 3.];
    match kind {
        GeometricCharacteristic::Position => {
            out.push(Primitive::Circle {
                center: c,
                radius: 1.6,
            });
            line(out, [c[0] - 2.4, c[1]], [c[0] + 2.4, c[1]]);
            line(out, [c[0], c[1] - 2.4], [c[0], c[1] + 2.4]);
        }
        GeometricCharacteristic::Flatness => {
            out.push(Primitive::Polyline {
                points: vec![
                    [at[0] + 0.7, at[1] + 1.5],
                    [at[0] + 4., at[1] + 1.5],
                    [at[0] + 5.3, at[1] + 4.5],
                    [at[0] + 2., at[1] + 4.5],
                    [at[0] + 0.7, at[1] + 1.5],
                ],
                dashed: false,
            });
        }
        GeometricCharacteristic::Straightness => line(out, [at[0] + 1., c[1]], [at[0] + 5., c[1]]),
        GeometricCharacteristic::Circularity => out.push(Primitive::Circle {
            center: c,
            radius: 1.8,
        }),
        GeometricCharacteristic::Cylindricity => {
            out.push(Primitive::Circle {
                center: c,
                radius: 1.5,
            });
            line(out, [at[0] + 0.5, at[1] + 1.], [at[0] + 2., at[1] + 5.]);
            line(out, [at[0] + 4., at[1] + 1.], [at[0] + 5.5, at[1] + 5.]);
        }
        GeometricCharacteristic::Parallelism => {
            line(out, [at[0] + 1., at[1] + 1.], [at[0] + 2.5, at[1] + 5.]);
            line(out, [at[0] + 3.5, at[1] + 1.], [at[0] + 5., at[1] + 5.]);
        }
        GeometricCharacteristic::Perpendicularity => {
            line(out, [at[0] + 1., at[1] + 1.5], [at[0] + 5., at[1] + 1.5]);
            line(out, [c[0], at[1] + 1.5], [c[0], at[1] + 5.]);
        }
        GeometricCharacteristic::Angularity => {
            line(out, [at[0] + 1., at[1] + 1.5], [at[0] + 5., at[1] + 1.5]);
            line(out, [at[0] + 1., at[1] + 1.5], [at[0] + 4.5, at[1] + 5.]);
        }
        GeometricCharacteristic::ProfileLine | GeometricCharacteristic::ProfileSurface => {
            let points = (0..=20)
                .map(|i| {
                    let a = std::f64::consts::PI * i as f64 / 20.;
                    [c[0] + 2. * a.cos(), at[1] + 2. + 2. * a.sin()]
                })
                .collect();
            out.push(Primitive::Polyline {
                points,
                dashed: false,
            });
            if *kind == GeometricCharacteristic::ProfileSurface {
                line(out, [at[0] + 1., at[1] + 2.], [at[0] + 5., at[1] + 2.]);
            }
        }
        GeometricCharacteristic::CircularRunout | GeometricCharacteristic::TotalRunout => {
            let arrows = if *kind == GeometricCharacteristic::TotalRunout {
                2
            } else {
                1
            };
            for i in 0..arrows {
                let a = [at[0] + 1. + i as f64 * 1.5, at[1] + 1.];
                let b = [at[0] + 3.5 + i as f64 * 1.5, at[1] + 5.];
                line(out, a, b);
                arrow(out, b, a);
            }
        }
    }
}
fn balloon(
    out: &mut Vec<Primitive>,
    entity: &Annotation,
    target: [f64; 2],
    offset: [f64; 2],
    number: u32,
    quantity: Option<usize>,
) -> Result<()> {
    let at = [target[0] + offset[0], target[1] + offset[1]];
    let value = number.to_string();
    let radius = (value.len() as f64 * 0.9 + 1.5).max(3.);
    let approach = entity
        .leader_points_mm
        .last()
        .map(|offset| [target[0] + offset[0], target[1] + offset[1]])
        .unwrap_or(target);
    let delta = [approach[0] - at[0], approach[1] - at[1]];
    let distance = delta[0].hypot(delta[1]);
    if distance <= radius + 1e-8 {
        return Err(invalid(
            "Move the balloon label away from its attachment point",
        ));
    }
    let rim = [
        at[0] + delta[0] * radius / distance,
        at[1] + delta[1] * radius / distance,
    ];
    leader_polyline(out, entity, target, rim, true);
    out.push(Primitive::Circle { center: at, radius });
    text(
        out,
        [at[0] - value.len() as f64 * 0.8, at[1] - 1.],
        2.7,
        value,
    );
    if let Some(quantity) = quantity {
        text(
            out,
            [at[0] + radius + 1., at[1] - 1.],
            2.5,
            format!("{quantity}x"),
        );
    }
    Ok(())
}
pub(crate) fn leader_polyline(
    out: &mut Vec<Primitive>,
    entity: &Annotation,
    target: [f64; 2],
    end: [f64; 2],
    arrowhead: bool,
) {
    let mut points = vec![target];
    points.extend(
        entity
            .leader_points_mm
            .iter()
            .map(|p| [target[0] + p[0], target[1] + p[1]]),
    );
    points.push(end);
    if arrowhead {
        arrow(out, target, points[1]);
    }
    out.push(Primitive::Polyline {
        points,
        dashed: false,
    });
}
fn annotation(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    view: &View,
    entity: &Annotation,
    out: &mut Vec<Primitive>,
) -> Result<()> {
    match &entity.kind {
        AnnotationKind::DatumTarget { target, symbol } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            crate::drawing_gdt::render_target(doc, sheet, entity, target, view.scale, symbol, out)?;
        }
        AnnotationKind::RevisionCloud { target, cloud } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            out.extend(cloud.render(sheet, entity, target)?);
        }
        AnnotationKind::GeometricToleranceFrame { target, frame } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            crate::drawing_gdt::render_frame(doc, sheet, entity, target, frame, out)?;
        }
        AnnotationKind::DimensionGroup { group } => {
            let projection = ViewProjection {
                rect_mm: [view.at[0], view.at[1], view.size[0], view.size[1]],
                model_center: view.center,
                scale: view.scale,
            };
            let (_, primitives) = crate::drawing_dimension_groups::render(
                doc,
                e,
                sheet,
                &view.name,
                &projection,
                group,
                entity.offset_mm,
            )?;
            out.extend(primitives);
        }
        AnnotationKind::SurfaceFinish { target, symbol } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            let configured = crate::configuration::effective(doc)?;
            symbol.render(
                &configured,
                &sheet.definition,
                target,
                entity.offset_mm,
                out,
            )?;
        }
        AnnotationKind::WeldSymbol { target, symbol } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            let configured = crate::configuration::effective(doc)?;
            symbol.render(
                &configured,
                &sheet.definition,
                target,
                entity.offset_mm,
                out,
            )?;
        }
        AnnotationKind::CutListBalloon {
            target,
            table,
            row_key,
            source,
            show_quantity,
        } => {
            let table = sheet
                .tables
                .iter()
                .find(|t| {
                    &t.id == table
                        && matches!(
                            t.kind,
                            crate::drawing_tables::TableKind::WeldmentCutList { .. }
                        )
                })
                .ok_or_else(|| invalid("Cut-list balloon requires its source cut-list table"))?;
            let content = crate::drawing_tables::content_for_sheet(doc, e, sheet, table)?;
            let row = content
                .entries
                .iter()
                .find(|row| &row.key == row_key)
                .ok_or_else(|| {
                    invalid("The cut-list balloon row is no longer active; reattach it explicitly")
                })?;
            if !crate::drawing_tables::row_contains_source(row, source) {
                return Err(invalid(
                    "Cut-list balloon member/occurrence no longer belongs to its selected row; reattach it explicitly",
                ));
            }
            if target.reference.as_ref().is_some_and(|reference| {
                !crate::drawing_tables::reference_matches_source(reference, source)
            }) {
                return Err(invalid(
                    "Cut-list balloon attachment belongs to another occurrence; choose a matching stock source or reattach its leader",
                ));
            }
            let point = if target.reference.is_some() {
                anchor(doc, e, sheet, target)?
            } else {
                crate::drawing_tables::balloon_source_point(doc, e, sheet, source)?
            };
            let target = view.screen(point)?;
            balloon(
                out,
                entity,
                target,
                entity.offset_mm,
                row.number,
                if *show_quantity {
                    Some(crate::drawing_tables::row_quantity(row))
                } else {
                    None
                },
            )?;
        }
        AnnotationKind::Balloon {
            target,
            table,
            row_key,
            definition,
            configuration,
            show_quantity,
        } => {
            let table = sheet
                .tables
                .iter()
                .find(|t| {
                    &t.id == table
                        && matches!(t.kind, crate::drawing_tables::TableKind::BillOfMaterials)
                })
                .ok_or_else(|| invalid("Balloon requires a BOM table on the same sheet"))?;
            let scoped = scoped_document(doc, e, sheet);
            let content = crate::drawing_tables::content(&scoped, e, table)?;
            if row_key.is_none()
                && content
                    .items
                    .iter()
                    .filter(|row| {
                        &row.definition == definition && &row.configuration == configuration
                    })
                    .count()
                    > 1
            {
                return Err(invalid(
                    "This component appears in multiple hierarchical/finish BOM rows; select an exact row for its balloon",
                ));
            }
            let row = content
                .items
                .iter()
                .find(|row| {
                    &row.definition == definition
                        && &row.configuration == configuration
                        && row_key.as_ref().is_none_or(|key| key == &row.key)
                })
                .ok_or_else(|| {
                    invalid("Balloon component/configuration is absent or excluded from its BOM")
                })?;
            if target.reference.as_ref().is_some_and(|reference| {
                !crate::drawing_tables::reference_matches_item(&scoped, reference, row)
            }) {
                return Err(invalid(
                    "BOM balloon attachment belongs to another component branch; select a matching row or reattach its leader",
                ));
            }
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            balloon(
                out,
                entity,
                target,
                entity.offset_mm,
                row.number,
                show_quantity.then_some(row.quantity),
            )?;
        }
        AnnotationKind::Leader {
            target,
            text: value,
        }
        | AnnotationKind::Datum {
            target,
            identifier: value,
        } => {
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            let at = [
                target[0] + entity.offset_mm[0],
                target[1] + entity.offset_mm[1],
            ];
            leader_polyline(out, entity, target, at, false);
            if matches!(entity.kind, AnnotationKind::Datum { .. }) {
                rectangle(
                    out,
                    [at[0], at[1] - 1.],
                    [value.chars().count() as f64 * 2.4 + 2., 5.],
                );
                out.push(Primitive::Polyline {
                    points: vec![
                        target,
                        [target[0] - 1., target[1] + 2.],
                        [target[0] + 1., target[1] + 2.],
                        target,
                    ],
                    dashed: false,
                });
            } else {
                let first = entity
                    .leader_points_mm
                    .first()
                    .map(|p| [target[0] + p[0], target[1] + p[1]])
                    .unwrap_or(at);
                arrow(out, target, first);
            }
            for (i, line) in value.lines().enumerate() {
                text(out, [at[0] + 1., at[1] - i as f64 * 4.], 3., line);
            }
        }
        AnnotationKind::GeometricTolerance {
            target,
            characteristic: kind,
            value,
            diameter,
            material,
            datums,
        } => {
            let value = crate::expression::value(doc, &sheet.definition, value, Dimension::Length)?;
            if value <= 0. {
                return Err(invalid("Geometric tolerance zone must be positive"));
            }
            let declared = sheet
                .entities
                .iter()
                .filter_map(|entity| match &entity.kind {
                    AnnotationKind::Datum { identifier, .. } => Some(identifier.as_str()),
                    AnnotationKind::DatumTarget { symbol, .. } => Some(symbol.identifier.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if datums.iter().any(|d| {
                !declared
                    .iter()
                    .any(|identifier| identifier.eq_ignore_ascii_case(d))
            }) {
                return Err(invalid(
                    "Geometric tolerance references an undeclared datum identifier",
                ));
            }
            let target = view.screen(anchor(doc, e, sheet, target)?)?;
            let at = [
                target[0] + entity.offset_mm[0],
                target[1] + entity.offset_mm[1],
            ];
            leader_polyline(out, entity, target, at, true);
            rectangle(out, at, [6., 6.]);
            characteristic(out, at, kind);
            let zone = format!("{}{value:.3}", if *diameter { "Ø" } else { "" });
            let size = zone.chars().count() as f64 * 1.8
                + 3.
                + if *material == MaterialCondition::Regardless {
                    0.
                } else {
                    4.
                };
            rectangle(out, [at[0] + 6., at[1]], [size, 6.]);
            text(out, [at[0] + 7., at[1] + 2.], 3., zone);
            if *material != MaterialCondition::Regardless {
                let c = [at[0] + 6. + size - 2.4, at[1] + 3.];
                out.push(Primitive::Circle {
                    center: c,
                    radius: 1.7,
                });
                text(
                    out,
                    [c[0] - 1., c[1] - 1.],
                    2.7,
                    if *material == MaterialCondition::Maximum {
                        "M"
                    } else {
                        "L"
                    },
                );
            }
            let mut x = at[0] + 6. + size;
            for datum in datums {
                let width = datum.chars().count() as f64 * 2. + 2.;
                rectangle(out, [x, at[1]], [width, 6.]);
                text(out, [x + 1., at[1] + 2.], 3., datum);
                x += width;
            }
        }
        AnnotationKind::CenterMark {
            curve,
            extension_mm,
        } => {
            let circle = circular(doc, e, sheet, curve)?;
            let center = view.screen(circle.center)?;
            let start = view.screen(circle.start)?;
            let radius = (start[0] - center[0]).hypot(start[1] - center[1]) + extension_mm;
            for (a, b) in [
                (
                    [center[0] - radius, center[1]],
                    [center[0] + radius, center[1]],
                ),
                (
                    [center[0], center[1] - radius],
                    [center[0], center[1] + radius],
                ),
            ] {
                out.push(Primitive::Polyline {
                    points: vec![a, b],
                    dashed: true,
                });
            }
        }
        AnnotationKind::CenterLine {
            first,
            second,
            extension_mm,
        } => {
            let a = view.screen(anchor(doc, e, sheet, first)?)?;
            let b = view.screen(anchor(doc, e, sheet, second)?)?;
            let d = [b[0] - a[0], b[1] - a[1]];
            let size = d[0].hypot(d[1]);
            if size < 1e-9 {
                return Err(invalid(
                    "Centerline endpoints coincide in the selected view",
                ));
            }
            out.push(Primitive::Polyline {
                points: vec![
                    [
                        a[0] - d[0] / size * extension_mm,
                        a[1] - d[1] / size * extension_mm,
                    ],
                    [
                        b[0] + d[0] / size * extension_mm,
                        b[1] + d[1] / size * extension_mm,
                    ],
                ],
                dashed: true,
            });
        }
    }
    Ok(())
}

pub fn sheets(doc: &Document, e: &Evaluation) -> Result<Vec<Sheet>> {
    let definitions = if doc.drawings.is_empty() {
        vec![Drawing {
            id: "automatic".into(),
            name: doc.name.clone(),
            definition: doc.root.clone(),
            views: vec![
                "front".into(),
                "top".into(),
                "right".into(),
                "isometric".into(),
            ],
            annotations: vec!["Units: millimeters".into()],
            paper_mm: [297., 210.],
            dimensions: vec![],
            title_block: std::collections::BTreeMap::new(),
            entities: vec![],
            tables: vec![],
            format: None,
            view_layouts: BTreeMap::new(),
            view_display_modes: ["front", "top", "right", "isometric"]
                .map(|key| {
                    (
                        key.into(),
                        crate::drawing_visibility::DisplayMode::HiddenLinesRemoved,
                    )
                })
                .into_iter()
                .collect(),
            view_visibility_geometry: BTreeMap::new(),
            exploded_view: None,
            display_state: None,
        }]
    } else {
        doc.drawings.clone()
    };
    let mut pages = vec![];
    for sheet in &definitions {
        for page in 0..crate::drawing_tables::page_count(sheet) {
            pages.push(build_sheet_page(doc, e, sheet, page)?);
        }
    }
    Ok(pages)
}
pub fn validate_evaluation(doc: &Document, e: &Evaluation) -> Result<()> {
    for sheet in &doc.drawings {
        for page in 0..crate::drawing_tables::page_count(sheet) {
            build_sheet_page(doc, e, sheet, page)?;
        }
    }
    Ok(())
}
pub fn build_sheet_page(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    page: u32,
) -> Result<Sheet> {
    if page == 0 {
        return build_sheet(doc, e, sheet);
    }
    if page >= crate::drawing_tables::page_count(sheet) {
        return Err(invalid("Requested continuation page is absent"));
    }
    build_continuation_sheet(doc, e, sheet, page, false)
}
fn table_page_geometries(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    page: u32,
    repair: bool,
    diagnostics: &mut BTreeMap<Id, String>,
) -> Result<Vec<(Id, crate::drawing_tables::TableGeometry)>> {
    let mut tables = vec![];
    for table in &sheet.tables {
        let geometries =
            match crate::drawing_tables::geometries_for_page(doc, e, sheet, table, page) {
                Ok(geometries) => geometries,
                Err(error) if repair => {
                    let message = format!("{} / {}: {error}", sheet.name, table.title);
                    diagnostics.insert(table.id.clone(), message.clone());
                    for segment in table.segments.iter().filter(|segment| segment.page == page) {
                        diagnostics.insert(segment.id.clone(), message.clone());
                    }
                    crate::drawing_tables::repair_placeholders(sheet, table, page)
                }
                Err(error) => return Err(error),
            };
        tables.extend(
            geometries
                .into_iter()
                .map(|geometry| (table.id.clone(), geometry)),
        );
    }
    let overlaps: Vec<_> = tables
        .iter()
        .enumerate()
        .flat_map(|(index, (_, geometry))| {
            tables
                .iter()
                .enumerate()
                .skip(index + 1)
                .filter(move |(_, (_, other))| {
                    rectangles_overlap(geometry.bounds_mm, other.bounds_mm)
                })
                .map(move |(other, _)| (index, other))
        })
        .collect();
    let title_overlaps: Vec<_> = title_bounds(sheet)
        .map(|title| {
            tables
                .iter()
                .enumerate()
                .filter(|(_, (_, geometry))| rectangles_overlap(title, geometry.bounds_mm))
                .map(|(index, _)| index)
                .collect()
        })
        .unwrap_or_default();
    let large_reserved: Vec<_> = if page == 0 && repair {
        let bounds = paper_bounds(sheet);
        let names = view_keys(sheet);
        let rows = names
            .len()
            .div_ceil(if names.len() == 1 { 1 } else { 2 })
            .max(1);
        let notes = 10. + (sheet.annotations.len() + sheet.title_block.len()) as f64 * 3.5;
        let notes = if sheet.format.is_some() {
            notes
        } else {
            notes.max(24.)
        };
        let maximum_footer = if sheet.format.is_some() {
            bounds[1][1] - 16. - rows as f64 * 24.
        } else {
            sheet.paper_mm[1] - 28. - rows as f64 * 24.
        };
        tables
            .iter()
            .enumerate()
            .filter(|(_, (_, geometry))| {
                geometry.reserve_space && geometry.bounds_mm[1][1] + notes > maximum_footer
            })
            .map(|(index, _)| index)
            .collect()
    } else {
        vec![]
    };
    let mut mark = |index: usize, message: String| -> Result<()> {
        if !repair {
            return Err(invalid(message));
        }
        let (id, geometry) = &mut tables[index];
        diagnostics.insert(id.clone(), message.clone());
        if let Some(segment) = &geometry.segment {
            diagnostics.insert(segment.clone(), message);
        }
        geometry.reserve_space = false;
        Ok(())
    };
    for index in large_reserved {
        mark(index,"This table leaves no room for model views; paginate it onto continuation pages or reduce/reposition it".into())?;
    }
    // Touching borders are harmless; rectangles_overlap excludes zero-area contact.
    for (index, other) in overlaps {
        mark(
            index,
            "Drawing table sections overlap; move one section to another position or page".into(),
        )?;
        mark(
            other,
            "Drawing table sections overlap; move one section to another position or page".into(),
        )?;
    }
    for index in title_overlaps {
        mark(
            index,
            "A drawing table overlaps the title block; move it above or beside the title block"
                .into(),
        )?;
    }
    Ok(tables)
}
fn build_continuation_sheet(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    page: u32,
    repair: bool,
) -> Result<Sheet> {
    validate_definition(sheet)?;
    let mut page_definition = sheet.clone();
    page_definition.id = format!("{}::continuation:{page}", sheet.id);
    page_definition.name = format!("{} · continuation {}", sheet.name, page + 1);
    let mut result = Sheet {
        id: page_definition.id.clone(),
        name: page_definition.name.clone(),
        paper_mm: sheet.paper_mm,
        primitives: format_geometry(doc, &page_definition)?,
        native_traces: vec![],
        views: BTreeMap::new(),
        projections: BTreeMap::new(),
        view_fidelity: BTreeMap::new(),
        source_info: DrawingSourceInfo::current(doc, e),
        display_state: None,
        groups: vec![],
        diagnostics: BTreeMap::new(),
    };
    if sheet.format.is_none() {
        rectangle(
            &mut result.primitives,
            [8., 8.],
            [sheet.paper_mm[0] - 16., sheet.paper_mm[1] - 16.],
        );
        text(
            &mut result.primitives,
            [12., sheet.paper_mm[1] - 13.],
            3.5,
            &page_definition.name,
        );
        text(
            &mut result.primitives,
            [12., 10.],
            2.5,
            format!("Source {} | page {}", sheet.name, page + 1),
        );
    }
    for (table, geometry) in
        table_page_geometries(doc, e, sheet, page, repair, &mut result.diagnostics)?
    {
        let primitive_start = result.primitives.len();
        let primitive_count = geometry.primitives.len();
        result.primitives.extend(geometry.primitives);
        result
            .groups
            .push(crate::drawing_interaction::PrimitiveGroup {
                target: if let Some(segment) = geometry.segment {
                    crate::drawing_interaction::Target::TableSegment { table, segment }
                } else {
                    crate::drawing_interaction::Target::Table { id: table }
                },
                view: None,
                primitive_start,
                primitive_count,
            });
    }
    Ok(result)
}
pub fn build_sheet(doc: &Document, e: &Evaluation, sheet: &Drawing) -> Result<Sheet> {
    build_sheet_mode(doc, e, sheet, false)
}
/// Broken associative annotations stay selectable on paper while they are being
/// repaired. Placeholder vectors are explicitly diagnostic and never exported.
pub fn build_sheet_preview(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    page: u32,
) -> Result<Sheet> {
    if page == 0 {
        build_sheet_mode(doc, e, sheet, true)
    } else {
        if page >= crate::drawing_tables::page_count(sheet) {
            return Err(invalid("Requested continuation page is absent"));
        }
        build_continuation_sheet(doc, e, sheet, page, true)
    }
}
/// Disposable layout/annotation geometry for synchronous interaction. It keeps
/// all source-reference and paper diagnostics, but omits surface visibility.
/// Production preview, validation and export must use the full sheet builders.
pub fn build_sheet_layout(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    page: u32,
) -> Result<Sheet> {
    let mut layout = sheet.clone();
    layout.view_display_modes.clear();
    build_sheet_preview(doc, e, &layout, page)
}
fn build_sheet_mode(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    repair: bool,
) -> Result<Sheet> {
    validate_definition(sheet)?;
    if sheet.paper_mm.iter().any(|v| !v.is_finite() || *v < 50.) {
        return Err(invalid("Drawing paper dimensions must be at least 50 mm"));
    }
    let mut result = Sheet {
        id: sheet.id.clone(),
        name: sheet.name.clone(),
        paper_mm: sheet.paper_mm,
        primitives: vec![],
        native_traces: vec![],
        views: BTreeMap::new(),
        projections: BTreeMap::new(),
        view_fidelity: BTreeMap::new(),
        source_info: DrawingSourceInfo::current(doc, e),
        display_state: None,
        groups: vec![],
        diagnostics: BTreeMap::new(),
    };
    let out = &mut result.primitives;
    if sheet.format.is_some() {
        out.extend(format_geometry(doc, sheet)?);
    } else {
        rectangle(
            out,
            [8., 8.],
            [sheet.paper_mm[0] - 16., sheet.paper_mm[1] - 16.],
        );
    }
    let bounds = paper_bounds(sheet);
    text(out, [bounds[0][0] + 4., bounds[1][1] - 8.], 5., &sheet.name);
    let tables = table_page_geometries(doc, e, sheet, 0, repair, &mut result.diagnostics)?;
    let display_scope = crate::drawing_display_states::scope(doc, sheet)?;
    let instances = crate::drawing_display_states::apply(
        display_scope.as_ref(),
        sheet_instances(doc, e, sheet)?,
    );
    result.display_state = display_scope
        .as_ref()
        .map(|scope| crate::drawing_display_states::info(scope, &instances));
    let exploded_lines = exploded_stage(doc, e, sheet)?
        .map(|s| {
            s.lines
                .iter()
                .filter(|line| {
                    display_scope.as_ref().is_none_or(|scope| {
                        crate::display_states::effect(scope, &line.path).visibility
                            != Some(crate::display_states::Visibility::Hidden)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut world = Vec::new();
    let mut curves = Vec::new();
    let mut triangles = Vec::new();
    let mut surface_extents = Vec::new();
    let mut line_dashes = Vec::new();
    let mut triangle_dashes = Vec::new();
    let visibility_requested = |geometry| {
        view_keys(sheet).iter().any(|key| {
            key != "section-z"
                && crate::drawing_visibility::mode_for(sheet, key)
                    != crate::drawing_visibility::DisplayMode::Wireframe
                && crate::drawing_visibility::geometry_for(sheet, key) == geometry
        })
    };
    let sampled_requested =
        visibility_requested(crate::drawing_visibility::GeometryMode::DisplayMesh);
    let native_requested = visibility_requested(crate::drawing_visibility::GeometryMode::Native);
    let mut visibility_scene = crate::drawing_visibility::Scene::default();
    let mut native_scene = crate::drawing_visibility::NativeScene::default();
    let mut source_faces = Vec::new();
    let mut native_storage_units = 0usize;
    for displayed in &instances {
        if displayed.effect.visibility == Some(crate::display_states::Visibility::Hidden) {
            continue;
        }
        let ghost = displayed.effect.visibility == Some(crate::display_states::Visibility::Ghost);
        let instance = &displayed.placement;
        let key = if !instance.mesh_key.is_empty() {
            instance.mesh_key.clone()
        } else if let Some(config) = &instance.configuration {
            format!("{}@{config}", instance.definition)
        } else {
            instance.definition.clone()
        };
        let Some(mesh) = e.meshes.get(&key) else {
            continue;
        };
        if !mesh.positions.is_empty()
            && mesh.bounds.iter().flatten().all(|value| value.is_finite())
            && (0..3).all(|axis| mesh.bounds[0][axis] <= mesh.bounds[1][axis])
        {
            // Native conservative bounds keep view fitting independent of
            // display tessellation and include outlines between source edges.
            surface_extents.extend((0..8).map(|corner| {
                transform(
                    std::array::from_fn(|axis| mesh.bounds[(corner >> axis) & 1][axis]),
                    instance.matrix,
                )
            }));
        }
        if sampled_requested {
            source_faces.extend(visibility_scene.add_mesh(
                mesh,
                instance.matrix,
                ghost,
                e.accuracy.modeling.linear_mm,
            )?);
        }
        if native_requested {
            native_scene.add_mesh(mesh, instance.matrix, ghost, e.accuracy.modeling.linear_mm)?;
        }
        for edge in &mesh.edges {
            line_dashes.push(ghost);
            world.push(
                edge.points
                    .iter()
                    .map(|p| transform(*p, instance.matrix))
                    .collect::<Vec<_>>(),
            );
            curves.push(edge.curve.as_ref().map(|curve| {
                crate::sketch_edit::Curve3 {
                    control_points: curve
                        .control_points
                        .iter()
                        .map(|p| transform(*p, instance.matrix))
                        .collect(),
                    ..curve.clone()
                }
            }));
        }
        triangle_dashes.extend(std::iter::repeat_n(ghost, mesh.indices.len() / 3));
        triangles.extend(
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|tri| tri.map(|v| transform(mesh.positions[v as usize], instance.matrix))),
        );
    }
    let names = view_keys(sheet);
    if names.len() > 16 {
        return Err(invalid("A sheet currently supports up to 16 views"));
    }
    if names
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != names.len()
    {
        return Err(invalid("Each named view can occur once per sheet"));
    }
    let orthographic = names.iter().any(|name| name == "front");
    let extra_views = names
        .iter()
        .filter(|name| !matches!(name.as_str(), "front" | "top" | "right" | "isometric"))
        .count();
    let columns = if orthographic {
        if names
            .iter()
            .any(|name| matches!(name.as_str(), "right" | "isometric"))
        {
            2
        } else {
            1
        }
    } else if names.len() == 1 {
        1
    } else {
        2
    };
    let extra_rows = if orthographic {
        extra_views.div_ceil(columns)
    } else {
        0
    };
    let rows = if orthographic {
        if names
            .iter()
            .any(|name| matches!(name.as_str(), "top" | "isometric"))
        {
            extra_rows + 2
        } else {
            extra_rows + 1
        }
    } else {
        names.len().div_ceil(columns)
    };
    let footer_notes_height = if sheet.format.is_some() {
        10. + (sheet.annotations.len() + sheet.title_block.len()) as f64 * 3.5
    } else {
        (10. + (sheet.annotations.len() + sheet.title_block.len()) as f64 * 3.5).max(24.)
    };
    let footer_base = if sheet.format.is_some() {
        title_bounds(sheet).map(|b| b[1][1]).unwrap_or(bounds[0][1])
    } else {
        0.
    };
    let footer_height = tables
        .iter()
        .filter(|(_, table)| table.reserve_space)
        .map(|(_, table)| table.bounds_mm[1][1] + footer_notes_height)
        .fold(footer_base + footer_notes_height, f64::max);
    let cell = if sheet.format.is_some() {
        [
            (bounds[1][0] - bounds[0][0] - 8.) / columns as f64,
            (bounds[1][1] - footer_height - 16.) / rows as f64,
        ]
    } else {
        [
            (sheet.paper_mm[0] - 24.) / columns as f64,
            (sheet.paper_mm[1] - footer_height - 28.) / rows as f64,
        ]
    };
    if cell[0] < 18. || cell[1] < 24. {
        return Err(invalid(
            "Paper is too small for its views and title notes; enlarge the sheet or reduce their count",
        ));
    }
    line(
        out,
        [bounds[0][0], footer_height + 2.],
        [bounds[1][0], footer_height + 2.],
    );
    let mut prepared = BTreeMap::new();
    let mut automatic_slots = BTreeMap::new();
    // The automatic layout is disposable presentation data. Saving/dragging a
    // resolved view copies its relationship into editable source; an explicit
    // unlinked layout remains the user's independent override.
    let mut effective = sheet.clone();
    for (index, name) in names.iter().enumerate() {
        let mut lines = world.clone();
        let mut view_curves = curves.clone();
        let mut view_dashes = line_dashes.clone();
        let mut note = if curves.iter().any(Option::is_none) {
            "Wireframe (unsupported curve types sampled)".to_string()
        } else {
            "Exact projected B-rep wireframe".to_string()
        };
        if name == "section-z" {
            let lo = triangles
                .iter()
                .flatten()
                .map(|p| p[2])
                .fold(f64::INFINITY, f64::min);
            let hi = triangles
                .iter()
                .flatten()
                .map(|p| p[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let z = if lo.is_finite() && hi.is_finite() {
                (lo + hi) / 2.
            } else {
                0.
            };
            lines.clear();
            view_dashes.clear();
            for (tri, ghost) in triangles.iter().zip(&triangle_dashes) {
                let mut points = vec![];
                for i in 0..3 {
                    let a = tri[i];
                    let b = tri[(i + 1) % 3];
                    if (a[2] - z) * (b[2] - z) < 0. {
                        let t = (z - a[2]) / (b[2] - a[2]);
                        points.push(std::array::from_fn(|i| a[i] + t * (b[i] - a[i])));
                    }
                }
                if points.len() == 2 {
                    lines.push(points);
                    view_dashes.push(*ghost);
                }
            }
            view_curves = vec![None; lines.len()];
            note = if triangles.is_empty() {
                "Section · no displayed bodies".into()
            } else {
                format!("Tessellated section Z={z:.3} mm")
            };
        }
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for p in lines
            .iter()
            .flatten()
            .chain(exploded_lines.iter().flat_map(|l| [&l.start_mm, &l.end_mm]))
            .chain(
                view_curves
                    .iter()
                    .flatten()
                    .flat_map(|c| c.control_points.iter()),
            )
            // Curved silhouettes can extend beyond native edges. Both layout
            // and classified views include these surface extents, so switching
            // line modes preserves view scale and dimension origins.
            .chain(surface_extents.iter().filter(|_| name != "section-z"))
        {
            let p = project(name, *p)?;
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        if !min[0].is_finite() {
            min = [-25., -25.];
            max = [25., 25.];
        }
        let (column, row) = if orthographic {
            match name.as_str() {
                "front" => (0, extra_rows),
                "top" => (0, extra_rows + 1),
                "right" => (1, extra_rows),
                "isometric" => (1, extra_rows + 1),
                _ => {
                    let extra_index = names[..index]
                        .iter()
                        .filter(|name| {
                            !matches!(name.as_str(), "front" | "top" | "right" | "isometric")
                        })
                        .count();
                    (
                        extra_index.rem_euclid(columns),
                        extra_rows - 1 - extra_index / columns,
                    )
                }
            }
        } else {
            (index.rem_euclid(columns), rows - 1 - index / columns)
        };
        let mut view = View {
            name: name.clone(),
            at: [
                bounds[0][0] + 4. + column as f64 * cell[0],
                footer_height + 6. + row as f64 * cell[1],
            ],
            size: [cell[0] - 8., cell[1] - 10.],
            center: [min[0] / 2. + max[0] / 2., min[1] / 2. + max[1] / 2.],
            scale: ((cell[0] - 8.) / (max[0] - min[0]).max(1.))
                .min((cell[1] - 10.) / (max[1] - min[1]).max(1.))
                * 0.78,
        };
        automatic_slots.insert(name.clone(), (column, row));
        if !sheet.view_layouts.contains_key(name) {
            effective.view_layouts.insert(
                name.clone(),
                ViewLayout {
                    rect_mm: [view.at[0], view.at[1], view.size[0], view.size[1]],
                    scale: None,
                    alignment: if orthographic
                        && sheet
                            .view_layouts
                            .get("front")
                            .and_then(|layout| layout.alignment.as_ref())
                            .is_none()
                    {
                        match name.as_str() {
                            "top" => Some(ViewAlignment {
                                parent: "front".into(),
                                axis: ViewAlignmentAxis::Vertical,
                                shared_scale: true,
                            }),
                            "right" => Some(ViewAlignment {
                                parent: "front".into(),
                                axis: ViewAlignmentAxis::Horizontal,
                                shared_scale: true,
                            }),
                            "isometric" => Some(ViewAlignment {
                                parent: "front".into(),
                                axis: ViewAlignmentAxis::Free,
                                shared_scale: true,
                            }),
                            _ => None,
                        }
                    } else {
                        None
                    },
                },
            );
        }
        if name == "detail-front" {
            view.scale *= 2.;
            note = "Detail wireframe (automatic 2× view scale)".into();
        }
        if let Some(layout) = sheet.view_layouts.get(name) {
            view.at = [layout.rect_mm[0], layout.rect_mm[1]];
            view.size = [layout.rect_mm[2], layout.rect_mm[3]];
            if view.at[1] < footer_height + 6. {
                let message = format!("View '{name}' overlaps reserved title/note/table space");
                if !repair {
                    return Err(invalid(message));
                }
                result.diagnostics.insert(view_id(sheet, name), message);
            }
            view.scale = layout.scale.unwrap_or_else(|| {
                (view.size[0] / (max[0] - min[0]).max(1.))
                    .min(view.size[1] / (max[1] - min[1]).max(1.))
                    * if name == "detail-front" { 1.56 } else { 0.78 }
            });
            if name == "detail-front" {
                note = "Detail wireframe (clipped to saved paper rectangle)".into();
            }
            if layout.scale.is_some()
                && !layout
                    .alignment
                    .as_ref()
                    .is_some_and(|alignment| alignment.shared_scale)
                && name != "detail-front"
                && ((max[0] - min[0]) * view.scale > view.size[0]
                    || (max[1] - min[1]) * view.scale > view.size[1])
            {
                let message =
                    format!("View '{name}' does not fit its fixed scale and paper rectangle");
                if !repair {
                    return Err(invalid(message));
                }
                result.diagnostics.insert(view_id(sheet, name), message);
            }
        }
        if !view.scale.is_finite() || view.scale <= 0. || view.center.iter().any(|v| !v.is_finite())
        {
            return Err(invalid(
                "A drawing view produced a nonfinite or nonpositive projection scale",
            ));
        }
        if view_dashes.iter().any(|ghost| *ghost) {
            note.push_str("; ghost components use dashed reference strokes");
        }
        prepared.insert(
            name.clone(),
            (view, lines, view_curves, note, min, max, view_dashes),
        );
    }
    // Reserve equal paper margins, rather than equal-sized model cells. A
    // shallow front view should not consume as much height as a tall top view
    // and force their common scale down an entire standard ratio.
    if sheet.view_layouts.is_empty() {
        let mut column_extents = vec![0f64; columns];
        let mut row_extents = vec![0f64; rows];
        for (name, (_, _, _, _, min, max, _)) in &prepared {
            let (column, row) = automatic_slots[name];
            column_extents[column] = column_extents[column].max((max[0] - min[0]).max(1.));
            row_extents[row] = row_extents[row].max((max[1] - min[1]).max(1.));
        }
        let area = [
            bounds[1][0] - bounds[0][0] - 8.,
            bounds[1][1] - footer_height - 13.,
        ];
        let margin = 24.;
        let gaps = [8., 10.];
        let limit = ((area[0] - margin * columns as f64 - gaps[0] * (columns - 1) as f64)
            / column_extents.iter().sum::<f64>())
        .min(
            (area[1] - margin * rows as f64 - gaps[1] * (rows - 1) as f64)
                / row_extents.iter().sum::<f64>(),
        );
        if !limit.is_finite() || limit <= 0. {
            return Err(invalid(
                "Paper is too small for clear view margins and captions; enlarge the sheet or reduce its views",
            ));
        }
        let scale = standard_scale(limit);
        let widths = column_extents
            .iter()
            .map(|extent| extent * scale + margin)
            .collect::<Vec<_>>();
        let heights = row_extents
            .iter()
            .map(|extent| extent * scale + margin)
            .collect::<Vec<_>>();
        let used = [
            widths.iter().sum::<f64>() + gaps[0] * (columns - 1) as f64,
            heights.iter().sum::<f64>() + gaps[1] * (rows - 1) as f64,
        ];
        let origin = [
            bounds[0][0] + 4. + (area[0] - used[0]) * 0.5,
            footer_height + 6. + (area[1] - used[1]) * 0.5,
        ];
        for (name, (view, _, _, _, _, _, _)) in &mut prepared {
            let (column, row) = automatic_slots[name];
            view.at = [
                origin[0] + widths[..column].iter().sum::<f64>() + gaps[0] * column as f64,
                origin[1] + heights[..row].iter().sum::<f64>() + gaps[1] * row as f64,
            ];
            view.size = [widths[column], heights[row]];
            view.scale = scale * if name == "detail-front" { 2. } else { 1. };
            let layout = effective
                .view_layouts
                .get_mut(name)
                .ok_or_else(|| invalid("Missing automatic view placement"))?;
            layout.rect_mm = [view.at[0], view.at[1], view.size[0], view.size[1]];
        }
    }
    validate_view_relationships(&effective)?;
    for name in &names {
        let layout = &effective.view_layouts[name];
        if layout.scale.is_some()
            || name == "detail-front"
            || layout
                .alignment
                .as_ref()
                .is_some_and(|alignment| alignment.shared_scale)
        {
            continue;
        }
        let limit = prepared
            .iter()
            .filter(|(candidate, _)| {
                let mut key = candidate.as_str();
                while let Some(parent) = effective
                    .view_layouts
                    .get(key)
                    .and_then(|layout| layout.alignment.as_ref())
                    .filter(|alignment| alignment.shared_scale)
                {
                    key = &parent.parent;
                }
                key == name
            })
            .map(|(_, (view, _, _, _, min, max, _))| {
                if sheet.view_layouts.is_empty() {
                    return view.scale;
                }
                (view.size[0] / (max[0] - min[0]).max(1.))
                    .min(view.size[1] / (max[1] - min[1]).max(1.))
                    * 0.78
            })
            .fold(f64::INFINITY, f64::min);
        if let Some(view) = prepared.get_mut(name) {
            view.0.scale = standard_scale(limit);
        }
    }
    let mut resolved = BTreeSet::new();
    for name in &names {
        resolve_view_alignment(&effective, name, &mut prepared, &mut resolved)?;
    }
    for name in &names {
        let (view, lines, view_curves, mut note, min, max, view_dashes) = prepared
            .remove(name)
            .ok_or_else(|| invalid("Missing prepared view"))?;
        let [x, y] = view.at;
        let [width, height] = view.size;
        if x < bounds[0][0]
            || y < footer_height + 6.
            || x + width > bounds[1][0]
            || y + height + 7. > bounds[1][1]
        {
            let message = format!(
                "View '{name}' falls outside its drawable paper area; move or resize the parent/child view"
            );
            if !repair {
                return Err(invalid(message));
            }
            result.diagnostics.insert(view_id(sheet, name), message);
        }
        if name != "detail-front"
            && ((max[0] - min[0]) * view.scale > width || (max[1] - min[1]) * view.scale > height)
        {
            let message = format!(
                "View '{name}' does not fit its scale; enlarge its paper rectangle or release shared scale"
            );
            if !repair {
                return Err(invalid(message));
            }
            result.diagnostics.insert(view_id(sheet, name), message);
        }
        result.views.insert(
            name.clone(),
            ViewLayout {
                rect_mm: [x, y, width, height],
                scale: Some(view.scale),
                alignment: effective
                    .view_layouts
                    .get(name)
                    .and_then(|layout| layout.alignment.clone()),
            },
        );
        result.projections.insert(
            name.clone(),
            ViewProjection {
                rect_mm: [x, y, width, height],
                model_center: view.center,
                scale: view.scale,
            },
        );
        let layout_invalid = result.diagnostics.contains_key(&view_id(sheet, name));
        let mode = crate::drawing_visibility::mode_for(sheet, name);
        let geometry = crate::drawing_visibility::geometry_for(sheet, name);
        let trace_view = TraceView {
            view: &view,
            mode,
            clip: name == "detail-front" || layout_invalid,
            tolerance_mm: e.accuracy.display_mm.max(e.accuracy.modeling.linear_mm),
        };
        let mut occlusion =
            if name != "section-z" && mode != crate::drawing_visibility::DisplayMode::Wireframe {
                Some(match geometry {
                    crate::drawing_visibility::GeometryMode::Native => {
                        crate::drawing_visibility::Projection::Native(
                            native_scene.project(name, e.accuracy)?,
                        )
                    }
                    crate::drawing_visibility::GeometryMode::DisplayMesh => {
                        crate::drawing_visibility::Projection::DisplayMesh(
                            visibility_scene.project(name, e.accuracy)?,
                        )
                    }
                })
            } else {
                None
            };
        let visibility = if name == "section-z" {
            crate::drawing_visibility::Fidelity::SectionIntersection {
                chord_tolerance_mm: e.accuracy.display_mm,
            }
        } else {
            occlusion.as_ref().map_or_else(
                || crate::drawing_visibility::Fidelity::Wireframe,
                |scene| scene.fidelity(),
            )
        };
        if occlusion.is_some() {
            note = match &visibility {
                crate::drawing_visibility::Fidelity::NativeSurfaces {
                    tolerance_mm,
                    curve_output_tolerance_mm,
                } => format!(
                    "{}; native surface visibility at {tolerance_mm:.8} mm; curve output bounded by {curve_output_tolerance_mm:.6} mm; native definitions retained",
                    mode.label()
                ),
                crate::drawing_visibility::Fidelity::Tessellated {
                    chord_tolerance_mm, ..
                } => format!(
                    "{}; sampled surface visibility and silhouettes ({chord_tolerance_mm:.4} mm chord tolerance); native rational edge carriers retained",
                    mode.label()
                ),
                _ => format!(
                    "{}; native planar trim visibility; exact rational edge carriers",
                    mode.label()
                ),
            };
            if geometry == crate::drawing_visibility::GeometryMode::DisplayMesh
                && curves.iter().any(Option::is_none)
            {
                note.push_str("; unsupported edge carriers sampled");
            }
            if name == "detail-front" {
                note.push_str("; detail clipping uses sampled paper curves");
            }
            if view_dashes.iter().any(|ghost| *ghost) {
                note.push_str("; ghost components remain reference strokes and do not occlude");
            }
        }
        result.view_fidelity.insert(
            name.clone(),
            ViewFidelity {
                approximate: visibility.approximate()
                    || ((occlusion.is_none()
                        || geometry == crate::drawing_visibility::GeometryMode::DisplayMesh)
                        && curves.iter().any(Option::is_none))
                    || name == "detail-front"
                    || layout_invalid,
                hidden_line_removal: mode
                    == crate::drawing_visibility::DisplayMode::HiddenLinesRemoved,
                visibility,
                description: if layout_invalid {
                    format!("Layout needs repair; preview clipped to saved rectangle. {note}")
                } else {
                    note
                },
            },
        );
        let view_start = out.len();
        text(out, [view.at[0], view.at[1] + view.size[1] + 4.], 3.5, name);
        text(
            out,
            [view.at[0], view.at[1] + view.size[1] + 1.],
            2.4,
            format!(
                "Scale {}{}",
                scale_label(view.scale),
                if orthographic && matches!(name.as_str(), "front" | "top" | "right") {
                    "; third-angle projection"
                } else {
                    ""
                }
            ),
        );
        for connector in &exploded_lines {
            let a = view.screen(connector.start_mm)?;
            let b = view.screen(connector.end_mm)?;
            if let Some([a, b]) = clipped_segment(a, b, &view) {
                out.push(Primitive::Polyline {
                    points: vec![a, b],
                    dashed: true,
                });
            }
        }
        for table in &sheet.tables {
            if let crate::drawing_tables::TableKind::Holes {
                view: coordinate_view,
                tag_prefix,
                ..
            } = &table.kind
                && coordinate_view == name
                && !result.diagnostics.contains_key(&table.id)
                && !table
                    .segments
                    .iter()
                    .any(|segment| result.diagnostics.contains_key(&segment.id))
            {
                let content = crate::drawing_tables::content_for_sheet(doc, e, sheet, table)?;
                let origin = view.screen(crate::drawing_tables::hole_datum_position(
                    doc, e, sheet, table,
                )?)?;
                if origin[0] >= view.at[0]
                    && origin[0] <= view.at[0] + view.size[0]
                    && origin[1] >= view.at[1]
                    && origin[1] <= view.at[1] + view.size[1]
                {
                    line(
                        out,
                        [origin[0] - 2., origin[1]],
                        [origin[0] + 2., origin[1]],
                    );
                    line(
                        out,
                        [origin[0], origin[1] - 2.],
                        [origin[0], origin[1] + 2.],
                    );
                    text(
                        out,
                        [origin[0] + 3., origin[1] - 3.],
                        2.5,
                        format!("{tag_prefix} DATUM"),
                    );
                }
                for row in content.entries {
                    if let Some(center) = row.position_mm {
                        let center = view.screen(center)?;
                        if center[0] >= view.at[0]
                            && center[0] <= view.at[0] + view.size[0]
                            && center[1] >= view.at[1]
                            && center[1] <= view.at[1] + view.size[1]
                        {
                            let at = [center[0] + 5., center[1] + 5.];
                            line(out, center, at);
                            text(out, at, 2.5, &row.cells[0]);
                        }
                    }
                }
            }
        }
        for (index, ((line, curve), dashed)) in lines
            .into_iter()
            .zip(view_curves)
            .zip(view_dashes)
            .enumerate()
        {
            if let Some(native) = occlusion.as_mut().and_then(|scene| scene.native()) {
                for trace in native.split_native(index)? {
                    trace_view.emit(
                        out,
                        &mut result.native_traces,
                        &mut native_storage_units,
                        trace,
                    )?;
                }
                continue;
            }
            let edge_curves = curve.map_or_else(
                || {
                    line.windows(2)
                        .map(|p| crate::drawing_visibility::line_curve([p[0], p[1]]))
                        .collect::<Vec<_>>()
                },
                |curve| vec![curve],
            );
            for curve in edge_curves {
                let pieces = if !dashed && let Some(scene) = &mut occlusion {
                    scene.split(&curve, source_faces.get(index).map_or(&[], Vec::as_slice))?
                } else {
                    vec![crate::drawing_visibility::Piece {
                        curve,
                        hidden: false,
                    }]
                };
                for piece in pieces {
                    if piece.hidden
                        && mode == crate::drawing_visibility::DisplayMode::HiddenLinesRemoved
                    {
                        continue;
                    }
                    emit_projected_curve(
                        out,
                        &view,
                        piece.curve,
                        dashed,
                        piece.hidden,
                        name == "detail-front" || layout_invalid,
                    )?;
                }
            }
        }
        if let Some(scene) = &mut occlusion {
            if let Some(native) = scene.native() {
                for trace in native.silhouettes()? {
                    trace_view.emit(
                        out,
                        &mut result.native_traces,
                        &mut native_storage_units,
                        trace,
                    )?;
                }
            }
            for (points, ghost) in scene.sampled_silhouettes() {
                let curve = crate::drawing_visibility::line_curve(points);
                let pieces = if ghost {
                    vec![crate::drawing_visibility::Piece {
                        curve,
                        hidden: false,
                    }]
                } else {
                    scene.split(&curve, &[])?
                };
                for piece in pieces {
                    if piece.hidden
                        && mode == crate::drawing_visibility::DisplayMode::HiddenLinesRemoved
                    {
                        continue;
                    }
                    emit_projected_curve(
                        out,
                        &view,
                        piece.curve,
                        ghost,
                        piece.hidden,
                        name == "detail-front" || layout_invalid,
                    )?;
                }
            }
        }
        for d in sheet.dimensions.iter().filter(|d| d.view == *name) {
            let start = out.len();
            if let Err(error) = dimension(doc, e, sheet, &view, d, out) {
                let message = format!("{} / Dimension {}: {error}", sheet.name, d.id);
                if !repair {
                    return Err(invalid(message));
                }
                out.truncate(start);
                let at = view
                    .screen(d.first)
                    .unwrap_or([view.at[0] + 4., view.at[1] + 4.]);
                text(out, at, 2.8, "DIMENSION: REATTACH REFERENCE");
                result.diagnostics.insert(d.id.clone(), message);
            }
            result
                .groups
                .push(crate::drawing_interaction::PrimitiveGroup {
                    target: crate::drawing_interaction::Target::Dimension { id: d.id.clone() },
                    view: Some(name.clone()),
                    primitive_start: start,
                    primitive_count: out.len() - start,
                });
        }
        for entity in sheet.entities.iter().filter(|entity| entity.view == *name) {
            let start = out.len();
            if let Err(error) = annotation(doc, e, sheet, &view, entity, out) {
                let message = format!("{} / Annotation {}: {error}", sheet.name, entity.id);
                if !repair {
                    return Err(invalid(message));
                }
                out.truncate(start);
                let at = [
                    view.at[0] + 4. + entity.offset_mm[0],
                    view.at[1] + 4. + entity.offset_mm[1],
                ];
                text(out, at, 2.8, "ANNOTATION: REATTACH REFERENCE");
                result.diagnostics.insert(entity.id.clone(), message);
            }
            result
                .groups
                .push(crate::drawing_interaction::PrimitiveGroup {
                    target: crate::drawing_interaction::Target::Annotation {
                        id: entity.id.clone(),
                    },
                    view: Some(name.clone()),
                    primitive_start: start,
                    primitive_count: out.len() - start,
                });
        }
        result
            .groups
            .push(crate::drawing_interaction::PrimitiveGroup {
                target: crate::drawing_interaction::Target::View { key: name.clone() },
                view: Some(name.clone()),
                primitive_start: view_start,
                primitive_count: out.len() - view_start,
            });
    }
    if sheet.dimensions.iter().any(|d| !names.contains(&d.view))
        || sheet.entities.iter().any(|a| !names.contains(&a.view))
    {
        return Err(invalid(
            "An annotation refers to a view that is absent from its sheet",
        ));
    }
    for (i, note) in sheet
        .annotations
        .iter()
        .chain(
            sheet
                .title_block
                .iter()
                .map(|(key, value)| format!("{key}: {value}"))
                .collect::<Vec<_>>()
                .iter(),
        )
        .enumerate()
    {
        text(
            out,
            [bounds[0][0] + 4., footer_height - 6. - i as f64 * 3.5],
            2.8,
            note,
        );
    }
    for (id, table) in tables {
        let start = out.len();
        out.extend(table.primitives);
        result
            .groups
            .push(crate::drawing_interaction::PrimitiveGroup {
                target: if let Some(segment) = table.segment {
                    crate::drawing_interaction::Target::TableSegment { table: id, segment }
                } else {
                    crate::drawing_interaction::Target::Table { id }
                },
                view: None,
                primitive_start: start,
                primitive_count: out.len() - start,
            });
    }
    if out.iter().any(|p| match p {
        Primitive::Polyline { points, .. } | Primitive::HiddenPolyline { points } => {
            points.iter().flatten().any(|v| !v.is_finite())
        }
        Primitive::Spline { curve, .. } | Primitive::HiddenSpline { curve } => curve
            .control_points
            .iter()
            .flatten()
            .any(|v| !v.is_finite()),
        Primitive::Text { at, size, .. } => at
            .iter()
            .chain(std::iter::once(size))
            .any(|v| !v.is_finite()),
        Primitive::Circle { center, radius } => center
            .iter()
            .chain(std::iter::once(radius))
            .any(|v| !v.is_finite()),
    }) {
        return Err(invalid(
            "An annotation produced non-finite paper coordinates",
        ));
    }
    Ok(result)
}

pub fn dxf(doc: &Document, e: &Evaluation) -> Result<String> {
    let sheets = sheets(doc, e)?;
    let mut out="0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n2\n0\nLTYPE\n2\nTCAD_REFERENCE\n70\n0\n3\nTCAD reference geometry\n72\n65\n73\n4\n40\n7\n49\n4\n74\n0\n49\n-1\n74\n0\n49\n1\n74\n0\n49\n-1\n74\n0\n0\nLTYPE\n2\nTCAD_HIDDEN\n70\n0\n3\nTCAD hidden model edges\n72\n65\n73\n2\n40\n3\n49\n2\n74\n0\n49\n-1\n74\n0\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n".to_string();
    let mut offset = 0.;
    for sheet in sheets {
        let layer = sheet.name.replace(['\n', '\r'], " ");
        for primitive in sheet.primitives {
            match primitive {
                Primitive::HiddenPolyline { points } => {
                    for pair in points.windows(2) {
                        let [a, b] = [pair[0], pair[1]];
                        out.push_str(&format!(
                            "0\nLINE\n8\n{layer}\n6\nTCAD_HIDDEN\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
                            a[0]+offset,a[1],b[0]+offset,b[1]
                        ));
                    }
                }
                Primitive::Polyline { points, dashed } => {
                    let segments = if dashed {
                        crate::drawing_strokes::reference_segments(&points)?
                    } else {
                        points.windows(2).map(|pair| [pair[0], pair[1]]).collect()
                    };
                    for [a, b] in segments {
                        out.push_str(&format!(
                            "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
                            a[0] + offset,
                            a[1],
                            b[0] + offset,
                            b[1]
                        ));
                    }
                }
                Primitive::Text { at, size, text } => out.push_str(&format!(
                    "0\nTEXT\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{size}\n1\n{}\n",
                    at[0] + offset,
                    at[1],
                    text.replace(['\n', '\r'], " ")
                )),
                Primitive::Spline { curve, dashed } => {
                    dxf_curve(
                        &mut out,
                        &curve,
                        &layer,
                        offset,
                        if dashed {
                            "TCAD_REFERENCE"
                        } else {
                            "CONTINUOUS"
                        },
                    );
                }
                Primitive::HiddenSpline { curve } => {
                    dxf_curve(&mut out, &curve, &layer, offset, "TCAD_HIDDEN");
                }
                Primitive::Circle { center, radius } => out.push_str(&format!(
                    "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{radius}\n",
                    center[0] + offset,
                    center[1]
                )),
            }
        }
        offset += sheet.paper_mm[0] + 20.;
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(out)
}

fn dxf_curve(
    out: &mut String,
    curve: &crate::sketch_edit::Curve2,
    layer: &str,
    offset: f64,
    linetype: &str,
) {
    if curve.degree == 1 && curve.control_points.len() == 2 {
        let [a, b] = [curve.control_points[0], curve.control_points[1]];
        out.push_str(&format!(
            "0\nLINE\n8\n{layer}\n6\n{linetype}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
            a[0] + offset,
            a[1],
            b[0] + offset,
            b[1]
        ));
        return;
    }
    out.push_str(&format!("0\nSPLINE\n8\n{layer}\n6\n{linetype}\n70\n12\n71\n{}\n72\n{}\n73\n{}\n74\n0\n210\n0\n220\n0\n230\n1\n",curve.degree,curve.knots.len(),curve.control_points.len()));
    for knot in &curve.knots {
        out.push_str(&format!("40\n{knot}\n"));
    }
    for weight in &curve.weights {
        out.push_str(&format!("41\n{weight}\n"));
    }
    for p in &curve.control_points {
        out.push_str(&format!("10\n{}\n20\n{}\n30\n0\n", p[0] + offset, p[1]));
    }
}

pub fn semantic_elements(doc: &Document, state: &str) -> Vec<crate::presentation::SemanticElement> {
    let mut out = Vec::new();
    for (index, sheet) in doc.drawings.iter().enumerate() {
        out.extend(crate::drawing_tables::semantic_elements(sheet, state));
        let mut commands = vec![
            "drawing.sheet-edit".into(),
            "drawing.sheet-preview".into(),
            "drawing.surface-finish-add".into(),
            "drawing.weld-symbol-add".into(),
            "drawing.sheet-duplicate".into(),
            "drawing.sheet-delete".into(),
            "drawing.sheet-template-save".into(),
            "drawing.sheet-format-load".into(),
        ];
        if index > 0 {
            commands.push("drawing.sheet-up".into());
        }
        if index + 1 < doc.drawings.len() {
            commands.push("drawing.sheet-down".into());
        }
        out.push(crate::presentation::SemanticElement {id:sheet.id.clone(),role:"drawing-sheet".into(),label:sheet.name.clone(),value:Some(format!("Sheet {} / {}",index+1,doc.drawings.len())),state:state.into(),commands,relationships:vec![sheet.definition.clone()].into_iter().chain(sheet.exploded_view.iter().cloned()).collect(),data:serde_json::json!({"contract":"tcad.drawing","version":CONTRACT_VERSION,"sheet_id":sheet.id,"sheet":sheet,"paper_units":"mm","template_contract":"tcad.drawing-sheet-template","template_version":FORMAT_VERSION})});
        out.push(crate::presentation::SemanticElement {id:format!("{}::format",sheet.id),role:"drawing-sheet-format".into(),label:"Sheet format and linked title block".into(),value:None,state:state.into(),commands:vec!["drawing.sheet-edit".into(),"drawing.sheet-format-load".into(),"drawing.sheet-template-save".into()],relationships:vec![sheet.id.clone()],data:serde_json::json!({"contract":"tcad.drawing-sheet-format","version":FORMAT_VERSION,"sheet_id":sheet.id,"format":sheet.format,"paper_units":"mm","vectors":format_geometry(doc,sheet).ok()})});
        for key in view_keys(sheet) {
            out.push(crate::presentation::SemanticElement {id:view_id(sheet,&key),role:"drawing-view".into(),label:key.clone(),value:None,state:state.into(),commands:vec!["drawing.sheet-edit".into(),"drawing.sheet-preview".into()],relationships:vec![sheet.id.clone()].into_iter().chain(sheet.view_layouts.get(&key).and_then(|layout|layout.alignment.as_ref()).map(|alignment|view_id(sheet,&alignment.parent))).collect(),data:serde_json::json!({"sheet_id":sheet.id,"view_key":key,"layout":sheet.view_layouts.get(&key),"display_mode":crate::drawing_visibility::mode_for(sheet,&key),"visibility_geometry":crate::drawing_visibility::geometry_for(sheet,&key),"paper_units":"mm","identity_scope":"sheet","exploded_view":sheet.exploded_view})});
        }
        for cell in sheet
            .format
            .iter()
            .flat_map(|f| &f.title_block)
            .flat_map(|b| &b.cells)
        {
            out.push(crate::presentation::SemanticElement {id:cell.id.clone(),role:"drawing-title-field".into(),label:cell.label.clone(),value:linked_text(doc,sheet,&cell.value).ok(),state:state.into(),commands:vec!["drawing.sheet-edit".into()],relationships:vec![sheet.id.clone(),sheet.definition.clone()],data:serde_json::json!({"contract":"tcad.drawing-sheet-format","version":FORMAT_VERSION,"sheet_id":sheet.id,"source":cell,"paper_units":"mm","linked_value":cell.value})});
        }
        for dimension in &sheet.dimensions {
            out.push(crate::presentation::SemanticElement {id:dimension.id.clone(),role:"drawing-dimension".into(),label:format!("{:?} dimension",dimension.options.kind),value:None,state:state.into(),commands:vec!["drawing.sheet-edit".into()],relationships:vec![sheet.id.clone()],data:serde_json::json!({"contract":"tcad.drawing","version":CONTRACT_VERSION,"sheet_id":sheet.id,"dimension":dimension,"measurement_intent":if dimension.options.second_curve.is_some() || dimension.options.kind == DimensionKind::Perpendicular{"parallel_edge_separation"}else if matches!(dimension.options.kind,DimensionKind::Diameter|DimensionKind::Radius|DimensionKind::HoleCallout){"circular_carrier"}else{"point_references"},"curve_references":{"first":dimension.options.curve,"second":dimension.options.second_curve},"choices":{"kind":["aligned","horizontal","vertical","perpendicular","angular","diameter","radius","hole_callout"],"tolerance":["none","symmetric","deviation","limits","basic"],"units":["millimeters","inches"]}})});
        }
        for entity in &sheet.entities {
            out.push(crate::presentation::SemanticElement {id:entity.id.clone(),role:"drawing-annotation".into(),label:entity.kind.label().into(),value:None,state:state.into(),commands:vec!["drawing.sheet-edit".into(),"drawing.annotation-delete".into()],relationships:vec![sheet.id.clone()],data:serde_json::json!({"contract":"tcad.drawing","version":CONTRACT_VERSION,"sheet_id":sheet.id,"annotation_id":entity.id,"annotation":entity,"symbol_contract":"tcad.drawing-symbol","symbol_version":SYMBOL_CONTRACT_VERSION,"actions":[{"command":"drawing.sheet-edit","label":"Edit annotation"},{"command":"drawing.annotation-delete","label":"Delete annotation"}]})});
            if matches!(
                entity.kind,
                AnnotationKind::DatumTarget { .. } | AnnotationKind::GeometricToleranceFrame { .. }
            ) && let Some(element) = out.last_mut()
            {
                element.data["symbol_contract"] = serde_json::json!("tcad.drawing-gdt");
                element.data["symbol_version"] =
                    serde_json::json!(crate::drawing_gdt::CONTRACT_VERSION);
            }
            if matches!(entity.kind, AnnotationKind::RevisionCloud { .. })
                && let Some(element) = out.last_mut()
            {
                element.data["symbol_contract"] = serde_json::json!("tcad.drawing-revision-cloud");
                element.data["symbol_version"] =
                    serde_json::json!(crate::drawing_revision::CONTRACT_VERSION);
            }
            if matches!(
                entity.kind,
                AnnotationKind::SurfaceFinish { .. } | AnnotationKind::WeldSymbol { .. }
            ) {
                out.extend(symbols::semantic_fields(sheet, entity, state));
            }
        }
    }
    out
}
