//! Versioned sketch-positioned drill features. No kernel geometry enters saved intent.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, presentation::SemanticElement,
};
use serde::{Deserialize, Serialize};
pub const CONTRACT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum End {
    #[default]
    Blind,
    ThroughAll,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Head {
    #[default]
    Simple,
    Counterbore {
        diameter: Expr,
        depth: Expr,
    },
    Countersink {
        diameter: Expr,
        angle: Expr,
    },
}
impl Head {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Simple => "Simple Hole",
            Self::Counterbore { .. } => "Counterbore",
            Self::Countersink { .. } => "Countersink",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    #[default]
    Right,
    Left,
}
impl Handedness {
    pub fn abbreviation(self) -> &'static str {
        match self {
            Self::Right => "RH",
            Self::Left => "LH",
        }
    }
}

/// Explicit cosmetic intent, independent of a thread standard or kernel helix.
/// The tap-drill diameter constructs the real bore; the nominal diameter and
/// pitch describe a thread without substituting fictitious thread surfaces.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CosmeticThread {
    pub nominal_diameter: Expr,
    pub pitch: Expr,
    pub tap_drill_diameter: Expr,
    #[serde(default)]
    pub handedness: Handedness,
    pub depth: Expr,
    #[serde(default)]
    pub end: End,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvaluatedCosmeticThread {
    pub nominal_diameter_mm: f64,
    pub pitch_mm: f64,
    pub tap_drill_diameter_mm: f64,
    pub handedness: Handedness,
    pub depth_mm: Option<f64>,
    pub end: End,
}
impl CosmeticThread {
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut values = vec![
            &self.nominal_diameter,
            &self.pitch,
            &self.tap_drill_diameter,
        ];
        if self.end == End::Blind {
            values.push(&self.depth);
        }
        values
    }

    pub fn evaluate(&self, doc: &Document, owner: &str) -> Result<EvaluatedCosmeticThread> {
        let length = |expression: &Expr, label: &str| -> Result<f64> {
            let value = crate::expression::value(doc, owner, expression, Dimension::Length)?;
            if !value.is_finite() || value <= 1e-7 {
                return Err(CadError::Invalid(format!(
                    "Tapped Hole: {label} must be finite and greater than 1e-7 mm"
                )));
            }
            Ok(value)
        };
        let nominal_diameter_mm = length(&self.nominal_diameter, "Nominal diameter")?;
        let tap_drill_diameter_mm = length(&self.tap_drill_diameter, "Tap-drill diameter")?;
        if nominal_diameter_mm <= tap_drill_diameter_mm + 1e-7 {
            return Err(CadError::Invalid(
                "Tapped Hole: Nominal diameter must exceed the tap-drill diameter".into(),
            ));
        }
        Ok(EvaluatedCosmeticThread {
            nominal_diameter_mm,
            pitch_mm: length(&self.pitch, "Pitch")?,
            tap_drill_diameter_mm,
            handedness: self.handedness,
            depth_mm: if self.end == End::Blind {
                Some(length(&self.depth, "Thread depth")?)
            } else {
                None
            },
            end: self.end,
        })
    }
}
impl EvaluatedCosmeticThread {
    /// Thread depth starts at the cylindrical bore below any entry head.
    /// Through cosmetics require an actually through-cut bore definition.
    pub fn validate_bore(
        &self,
        bore_end: End,
        bore_depth_mm: f64,
        head_depth_mm: f64,
    ) -> Result<()> {
        if !bore_depth_mm.is_finite()
            || !head_depth_mm.is_finite()
            || head_depth_mm < 0.
            || bore_depth_mm <= head_depth_mm + 1e-7
        {
            return Err(CadError::Invalid(
                "Tapped Hole: Entry head must leave positive cylindrical bore length".into(),
            ));
        }
        if self.end == End::ThroughAll && bore_end != End::ThroughAll {
            return Err(CadError::Invalid(
                "Tapped Hole: Through thread intent requires a Through All bore".into(),
            ));
        }
        if self
            .depth_mm
            .is_some_and(|depth| !depth.is_finite() || depth <= 1e-7)
            || (self.end == End::Blind) != self.depth_mm.is_some()
        {
            return Err(CadError::Invalid(
                "Tapped Hole: Cosmetic depth must be positive for Blind and absent for Through All"
                    .into(),
            ));
        }
        if self
            .depth_mm
            .is_some_and(|depth| depth > bore_depth_mm - head_depth_mm + 1e-7)
        {
            return Err(CadError::Invalid(
                "Tapped Hole: Thread depth exceeds the cylindrical bore depth below the entry head"
                    .into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub body: NodeRef,
    pub positions: NodeRef,
    /// Explicit stable sketch point identities, including parametrically constrained points.
    pub points: Vec<Id>,
    pub diameter: Expr,
    pub depth: Expr,
    #[serde(default)]
    pub end: End,
    #[serde(default)]
    pub head: Head,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<Box<CosmeticThread>>,
    /// Normal CAD entry direction is into the selected sketch face.
    #[serde(default)]
    pub reverse: bool,
    #[serde(default = "zero")]
    pub start_offset: Expr,
}
fn zero() -> Expr {
    "0 mm".into()
}
impl Definition {
    pub fn name(&self) -> &'static str {
        if self.thread.is_some() {
            "Tapped Hole"
        } else {
            self.head.name()
        }
    }
    pub fn bore_diameter(&self) -> &Expr {
        self.thread
            .as_ref()
            .map(|thread| &thread.tap_drill_diameter)
            .unwrap_or(&self.diameter)
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut values = vec![&self.start_offset];
        if let Some(thread) = &self.thread {
            values.extend(thread.expressions());
        } else {
            values.push(&self.diameter);
        }
        if self.end == End::Blind {
            values.push(&self.depth);
        }
        match &self.head {
            Head::Simple => {}
            Head::Counterbore { diameter, depth } => values.extend([diameter, depth]),
            Head::Countersink { diameter, angle } => values.extend([diameter, angle]),
        }
        values
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        vec![self.body.clone(), self.positions.clone()]
    }
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let mut fields = vec![
        (
            "diameter",
            if definition.thread.is_some() {
                "Tap-Drill Diameter"
            } else {
                "Hole Diameter"
            },
            definition.bore_diameter(),
            Dimension::Length,
        ),
        (
            "start_offset",
            "Start Offset",
            &definition.start_offset,
            Dimension::Length,
        ),
    ];
    if definition.end == End::Blind {
        fields.push(("depth", "Hole Depth", &definition.depth, Dimension::Length));
    }
    if let Some(thread) = &definition.thread {
        fields.extend([
            (
                "thread_nominal",
                "Thread Nominal Diameter",
                &thread.nominal_diameter,
                Dimension::Length,
            ),
            (
                "thread_pitch",
                "Thread Pitch",
                &thread.pitch,
                Dimension::Length,
            ),
        ]);
        if thread.end == End::Blind {
            fields.push((
                "thread_depth",
                "Thread Depth Below Entry Head",
                &thread.depth,
                Dimension::Length,
            ));
        }
    }
    match &definition.head {
        Head::Simple => {}
        Head::Counterbore { diameter, depth } => fields.extend([
            (
                "head_diameter",
                "Counterbore Diameter",
                diameter,
                Dimension::Length,
            ),
            ("head_depth", "Counterbore Depth", depth, Dimension::Length),
        ]),
        Head::Countersink { diameter, angle } => fields.extend([
            (
                "head_diameter",
                "Countersink Diameter",
                diameter,
                Dimension::Length,
            ),
            ("head_angle", "Included Angle", angle, Dimension::Angle),
        ]),
    }
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let mut out:Vec<_>=fields.into_iter().map(|(field,label,expression,dimension)|SemanticElement {id:format!("{node}::holes::{field}"),role:"feature-field".into(),label:label.into(),value:Some(expression.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.drilled-holes","version":CONTRACT_VERSION,"target":target,"field":field,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true})}).collect();
    out.push(SemanticElement {id:format!("{node}::holes::options"),role:"feature-options".into(),label:"Hole Options".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:definition.dependencies().iter().map(|r|r.node.clone()).collect(),data:serde_json::json!({"contract":"tcad.drilled-holes","version":CONTRACT_VERSION,"target":target,"source":definition,"styles":["simple","counterbore","countersink"],"ends":["blind","through_all"],"positions":"explicit_solved_sketch_point_ids","bottom":"flat","threaded":definition.thread.is_some(),"thread_geometry":"cosmetic","modeled_thread_surfaces":false,"thread_depth_origin":"cylindrical_entry_below_head","evaluated_thread":definition.thread.as_ref().and_then(|thread|thread.evaluate(doc,owner).ok()),"standards_table":false,"editable":true})});
    out
}
