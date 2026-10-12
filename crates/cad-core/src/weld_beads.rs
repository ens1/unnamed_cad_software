//! Associative deposited weld metal, independent of native solids and renderers.
use crate::{CadError, Dimension, Document, Expr, Id, NodeRef, Result, TopologyRef};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SEGMENTS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeamSource {
    ModelEdge {
        body: NodeRef,
        reference: Box<TopologyRef>,
        /// Complete occurrence path relative to the assembly owning this weld.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        path: Vec<Id>,
    },
    SketchMember {
        profile: NodeRef,
        member: Id,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        path: Vec<Id>,
    },
    ReferenceCurve {
        feature: NodeRef,
        /// Absent selects the complete reference path; present selects one native piece.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        piece: Option<Id>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        path: Vec<Id>,
    },
}
impl SeamSource {
    pub fn dependency(&self) -> &NodeRef {
        match self {
            Self::ModelEdge { body, .. } => body,
            Self::SketchMember { profile, .. } => profile,
            Self::ReferenceCurve { feature, .. } => feature,
        }
    }
    pub fn path(&self) -> &[Id] {
        match self {
            Self::ModelEdge { path, .. }
            | Self::SketchMember { path, .. }
            | Self::ReferenceCurve { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceSource {
    pub body: NodeRef,
    pub reference: Box<TopologyRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Id>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Equal-leg root with a circular crown. Size is the leg length.
    #[default]
    Fillet,
    /// Circular cap above the seam. Size is the toe-to-toe width.
    Butt,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pattern {
    #[default]
    Continuous,
    Intermittent {
        length: Expr,
        gap: Expr,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    pub seam: SeamSource,
    /// Zero infers adjacent model faces, or uses the authored seam's direction.
    /// Two explicit faces define a fillet joint; one defines a butt cap support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supports: Vec<FaceSource>,
    #[serde(default)]
    pub profile: Profile,
    pub size: Expr,
    /// Circular crown rise above the chord, in physical length units.
    pub reinforcement: Expr,
    /// Used for an explicitly authored sketch/reference seam without joint faces.
    pub direction: [f64; 3],
    pub roll: Expr,
    #[serde(default)]
    pub pattern: Pattern,
    pub start_offset: Expr,
    pub end_offset: Expr,
    /// Optical finish and engineering density/material are independent assignments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Id>,
}
impl Definition {
    pub fn new(id: Id, name: String, seam: SeamSource) -> Self {
        Self {
            id,
            name,
            suppressed: false,
            seam,
            supports: vec![],
            profile: Profile::Fillet,
            size: Expr::from("3 mm"),
            reinforcement: Expr::from("0.5 mm"),
            direction: [0., 0., 1.],
            roll: Expr::from("0 deg"),
            pattern: Pattern::Continuous,
            start_offset: Expr::from("0 mm"),
            end_offset: Expr::from("0 mm"),
            finish: None,
            material: None,
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut out = vec![self.seam.dependency().clone()];
        out.extend(self.supports.iter().map(|face| face.body.clone()));
        out.sort();
        out.dedup();
        out
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = vec![
            &self.size,
            &self.reinforcement,
            &self.roll,
            &self.start_offset,
            &self.end_offset,
        ];
        if let Pattern::Intermittent { length, gap } = &self.pattern {
            out.extend([length, gap]);
        }
        out
    }
    pub fn validate(&self) -> Result<()> {
        let portable = |id: &str| {
            !id.is_empty()
                && id.len() <= 256
                && !matches!(id, "." | "..")
                && !id
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        };
        if !portable(&self.id) || self.name.trim().is_empty() || self.name.len() > 256 {
            return Err(invalid(
                "A weld needs a portable stable identity and a nonempty name",
            ));
        }
        let paths = std::iter::once(self.seam.path())
            .chain(self.supports.iter().map(|f| f.path.as_slice()));
        if paths
            .into_iter()
            .any(|path| path.len() > 256 || path.iter().any(|id| !portable(id)))
        {
            return Err(invalid(
                "Occurrence paths need complete portable instance identities",
            ));
        }
        if self.direction.iter().any(|v| !v.is_finite())
            || self
                .direction
                .iter()
                .fold(0_f64, |length, v| length.hypot(*v))
                <= 1e-12
        {
            return Err(invalid(
                "The bead orientation needs a finite nonzero direction",
            ));
        }
        if matches!(&self.seam, SeamSource::ModelEdge {reference,..} if reference.topology != "edge")
            || self
                .supports
                .iter()
                .any(|face| face.reference.topology != "face")
        {
            return Err(invalid(
                "Seams need saved edge ancestry and supports need saved face ancestry",
            ));
        }
        if self.supports.len()
            > if self.profile == Profile::Fillet {
                2
            } else {
                1
            }
        {
            return Err(invalid(
                "A fillet joint supports at most two faces and a butt bead supports one; complete contact is checked by geometry evaluation before Apply",
            ));
        }
        if matches!(&self.seam, SeamSource::SketchMember {member,..} if !portable(member))
            || matches!(&self.seam, SeamSource::ReferenceCurve {piece:Some(piece),..} if !portable(piece))
        {
            return Err(invalid(
                "The seam member or curve piece identity is invalid",
            ));
        }
        Ok(())
    }
    pub fn values(&self, doc: &Document, owner: &str) -> Result<Values> {
        self.validate()?;
        let length = |e: &Expr| crate::expression::value(doc, owner, e, Dimension::Length);
        let size = length(&self.size)?;
        let reinforcement = length(&self.reinforcement)?;
        let start_offset = length(&self.start_offset)?;
        let end_offset = length(&self.end_offset)?;
        let roll = crate::expression::value(doc, owner, &self.roll, Dimension::Angle)?;
        if !size.is_finite()
            || size <= 1e-7
            || size > 1e6
            || !reinforcement.is_finite()
            || reinforcement < 0.
            || reinforcement > size
            || (self.profile == Profile::Butt && reinforcement <= 1e-7)
            || [start_offset, end_offset]
                .into_iter()
                .any(|v| !v.is_finite() || !(0. ..=1e12).contains(&v))
            || !roll.is_finite()
        {
            return Err(invalid(
                "Size must be positive; crown rise and end offsets must be nonnegative; a butt bead needs a positive crown",
            ));
        }
        let stitch = match &self.pattern {
            Pattern::Continuous => None,
            Pattern::Intermittent { length: l, gap } => {
                let pair = [length(l)?, length(gap)?];
                if pair
                    .into_iter()
                    .any(|v| !v.is_finite() || !(1e-7..=1e12).contains(&v))
                {
                    return Err(invalid(
                        "Intermittent welds need positive stitch length and clear gap",
                    ));
                }
                Some(pair)
            }
        };
        if self
            .finish
            .as_ref()
            .is_some_and(|id| !doc.finishes.contains_key(id))
            || self
                .material
                .as_ref()
                .is_some_and(|id| !doc.materials.contains_key(id))
        {
            return Err(invalid(
                "The bead's finish or engineering material assignment is missing",
            ));
        }
        Ok(Values {
            size_mm: size,
            reinforcement_mm: reinforcement,
            roll_rad: roll,
            start_offset_mm: start_offset,
            end_offset_mm: end_offset,
            stitch_mm: stitch,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Values {
    pub size_mm: f64,
    pub reinforcement_mm: f64,
    pub roll_rad: f64,
    pub start_offset_mm: f64,
    pub end_offset_mm: f64,
    pub stitch_mm: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedSegment {
    pub id: Id,
    pub start_mm: f64,
    pub end_mm: f64,
    pub length_mm: f64,
    pub volume_mm3: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub version: u32,
    pub id: Id,
    pub source: NodeRef,
    pub seam_length_mm: f64,
    pub deposited_length_mm: f64,
    pub volume_mm3: f64,
    pub section_area_mm2: f64,
    pub segment_count: usize,
    pub segments: Vec<EvaluatedSegment>,
    pub real_solid: bool,
    pub finish: Option<Id>,
    pub material: Option<Id>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Weld bead: {message}"))
}

pub fn validate_parts(doc: &Document) -> Result<()> {
    for part in doc.parts.values() {
        for feature in &part.features {
            let crate::Operation::WeldBead { definition } = &feature.operation else {
                continue;
            };
            definition.validate()?;
            if definition.id != feature.id
                || !definition.seam.path().is_empty()
                || definition.supports.iter().any(|face| !face.path.is_empty())
            {
                return Err(invalid(
                    "A part weld's identity must match its feature and every source must use the part-local frame",
                ));
            }
            if definition
                .dependencies()
                .iter()
                .any(|source| source.definition != part.id)
            {
                return Err(invalid(
                    "A part weld uses sources in its own part; use an associative local reference feature or an assembly weld for another component",
                ));
            }
            if definition
                .finish
                .as_ref()
                .is_some_and(|id| !doc.finishes.contains_key(id))
                || definition
                    .material
                    .as_ref()
                    .is_some_and(|id| !doc.materials.contains_key(id))
            {
                return Err(invalid(
                    "The part weld's finish or engineering material assignment is missing",
                ));
            }
        }
    }
    Ok(())
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let mut quantities = vec![
        (
            "size",
            if definition.profile == Profile::Fillet {
                "Leg Size"
            } else {
                "Bead Width"
            },
            &definition.size,
            Dimension::Length,
        ),
        (
            "reinforcement",
            "Crown Rise",
            &definition.reinforcement,
            Dimension::Length,
        ),
        (
            "roll",
            "Authored Seam Roll",
            &definition.roll,
            Dimension::Angle,
        ),
        (
            "start_offset",
            "Start Offset",
            &definition.start_offset,
            Dimension::Length,
        ),
        (
            "end_offset",
            "End Offset",
            &definition.end_offset,
            Dimension::Length,
        ),
    ];
    if let Pattern::Intermittent { length, gap } = &definition.pattern {
        quantities.extend([
            ("stitch_length", "Stitch Length", length, Dimension::Length),
            ("stitch_gap", "Clear Gap", gap, Dimension::Length),
        ]);
    }
    let mut fields: Vec<_> = quantities.into_iter().map(|(field,label,expr,dimension)| crate::presentation::SemanticElement {
        id:format!("{node}::weld::{field}"), role:"feature-field".into(), label:label.into(), value:Some(expr.0.clone()), state:state.into(), commands:vec!["feature.edit".into()], relationships:vec![node.into()],
        data:serde_json::json!({"contract":"tcad.weld-bead","version":CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":expr,"evaluated":crate::expression::value(doc,owner,expr,dimension).ok(),"editable":true}),
    }).collect();
    fields.push(crate::presentation::SemanticElement {id:format!("{node}::weld::options"),role:"feature-options".into(),label:definition.name.clone(),value:None,state:state.into(),commands:vec!["feature.edit".into(),"weld.bead".into()],relationships:definition.dependencies().into_iter().map(|source| source.node).collect(),
        data:serde_json::json!({"contract":"tcad.weld-bead","version":CONTRACT_VERSION,"source":definition,"editable":true,"capabilities":{"real_solid":true,"seam_sources":["construction_model_edge","sketch_member","native_reference_curve"],"native_carriers":["collinear_lines","coplanar_coaxial_circular_arcs"],"patterns":["continuous","intermittent"],"profiles":["equal_leg_fillet_with_circular_crown","butt_circular_cap"],"assemblies":"saved_occurrence_paths","independent_finish_and_material":true,"fusion":false,"cosmetic_mesh_authority":false}})});
    fields
}
