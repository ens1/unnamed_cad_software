//! Versioned body-editing intent, independent of kernel and renderer objects.
use crate::{
    CadError, Dimension, Document, Expr, Result, expression,
    presentation::SemanticElement,
    reference_geometry::{AxisSource, OriginAxis, PointSource, Resolver},
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 3;

/// Disposable owner-local, f64 viewport information from evaluated native directions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatternGuide {
    #[serde(default)]
    pub document: crate::Id,
    pub origin_mm: [f64; 3],
    pub steps_mm: [[f64; 3]; 2],
    pub counts: [usize; 2],
}
/// Associative straight direction, measured in the pattern owner's local frame.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PatternDirection {
    pub axis: AxisSource,
    pub spacing: Expr,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<PatternOccurrence>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PatternOccurrence {
    /// Complete root-to-reference occurrence path; never a leaf ID alone.
    pub path: Vec<crate::Id>,
    /// Complete root-to-pattern-owner occurrence path.
    pub owner_path: Vec<crate::Id>,
}
impl Default for PatternDirection {
    fn default() -> Self {
        Self {
            axis: AxisSource::Origin {
                axis: OriginAxis::X,
            },
            spacing: "30 mm".into(),
            reverse: false,
            occurrence: None,
        }
    }
}
impl PatternDirection {
    pub fn validate(&self) -> Result<()> {
        if matches!(
            self.axis,
            AxisSource::CircleEdge { .. }
                | AxisSource::SketchCircle { .. }
                | AxisSource::FaceAxis { .. }
        ) {
            return Err(CadError::Unsupported("Linear Pattern: choose a straight sketch line, straight model edge, or reference axis; curved edges and face normals are ambiguous directions".into()));
        }
        if self
            .occurrence
            .as_ref()
            .is_some_and(|p| p.path.is_empty() || p.owner_path.is_empty())
        {
            return Err(CadError::Invalid(
                "Linear Pattern: both complete reference and owner occurrence paths are required"
                    .into(),
            ));
        }
        Ok(())
    }
    pub fn dependencies(&self) -> Vec<crate::NodeRef> {
        self.axis.dependencies()
    }
    pub fn resolve(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<[f64; 3]> {
        self.validate()?;
        let mut direction = nalgebra::Vector3::from(self.axis.resolve(resolver)?.direction);
        if let Some(occurrence) = &self.occurrence {
            let placements = doc.placed_parts()?;
            let reference = placements
                .iter()
                .find(|p| p.path == occurrence.path)
                .ok_or_else(|| {
                    CadError::Invalid(
                        "Linear Pattern: reference occurrence is suppressed or missing".into(),
                    )
                })?;
            let _target = placements.iter().find(|p| p.path == occurrence.owner_path && p.definition == owner).ok_or_else(|| CadError::Invalid("Linear Pattern: owner occurrence is suppressed, missing, or has changed definition".into()))?;
            if self
                .dependencies()
                .iter()
                .any(|r| r.definition != reference.definition)
            {
                return Err(CadError::Invalid(
                    "Linear Pattern: reference geometry and complete occurrence path disagree"
                        .into(),
                ));
            }
            let matrix = |a: [[f64; 4]; 4]| nalgebra::Matrix4::from_fn(|i, j| a[i][j]);
            let relative = matrix(resolver.occurrence_placement(&occurrence.owner_path)?)
                .try_inverse()
                .ok_or_else(|| {
                    CadError::Invalid("Linear Pattern: owner placement is singular".into())
                })?
                * matrix(resolver.occurrence_placement(&occurrence.path)?);
            direction = relative.fixed_view::<3, 3>(0, 0) * direction;
        } else if self.dependencies().iter().any(|r| r.definition != owner) {
            return Err(CadError::Invalid("Linear Pattern: a direction on another component requires a complete occurrence path".into()));
        }
        let spacing = expression::value(doc, owner, &self.spacing, Dimension::Length)?;
        if !spacing.is_finite() || spacing <= 0. {
            return Err(CadError::Invalid("Linear Pattern: spacing must be a positive finite length; use Reverse to change direction".into()));
        }
        let direction = direction.try_normalize(1e-12).ok_or_else(|| {
            CadError::Invalid("Linear Pattern: direction is zero or invalid".into())
        })? * spacing
            * if self.reverse { -1. } else { 1. };
        Ok(direction.into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LinearPatternDirection {
    pub count: usize,
    pub step: [Expr; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<PatternDirection>,
}
impl Default for LinearPatternDirection {
    fn default() -> Self {
        Self {
            count: 2,
            step: ["0 mm".into(), "30 mm".into(), "0 mm".into()],
            direction: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PatternOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<PatternDirection>,
    pub second_direction: Option<LinearPatternDirection>,
    /// Zero-based [direction-1, direction-2] instance coordinates; the seed cannot be skipped.
    pub skipped: Vec<[usize; 2]>,
    /// Treat the saved angle as the total circular extent, with equal spacing.
    pub angle_is_total: bool,
    pub rotate_instances: bool,
    /// Associative seed point used when circular instances keep their orientation.
    pub seed: PointSource,
}
impl Default for PatternOptions {
    fn default() -> Self {
        Self {
            direction: None,
            second_direction: None,
            skipped: vec![],
            angle_is_total: false,
            rotate_instances: true,
            seed: PointSource::default(),
        }
    }
}
impl PatternOptions {
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = self
            .direction
            .iter()
            .map(|d| &d.spacing)
            .collect::<Vec<_>>();
        if let Some(second) = &self.second_direction {
            if let Some(direction) = &second.direction {
                out.push(&direction.spacing);
            } else {
                out.extend(second.step.iter());
            }
        }
        if !self.rotate_instances {
            out.extend(self.seed.expressions());
        }
        out
    }
    pub fn dependencies(&self) -> Vec<crate::NodeRef> {
        self.direction
            .iter()
            .chain(
                self.second_direction
                    .iter()
                    .filter_map(|d| d.direction.as_ref()),
            )
            .flat_map(PatternDirection::dependencies)
            .collect()
    }
    pub fn requires_placement(&self) -> bool {
        self.direction
            .iter()
            .chain(
                self.second_direction
                    .iter()
                    .filter_map(|d| d.direction.as_ref()),
            )
            .any(|d| d.occurrence.is_some())
    }
    pub fn validate(&self, count: usize) -> Result<usize> {
        for direction in self.direction.iter().chain(
            self.second_direction
                .iter()
                .filter_map(|d| d.direction.as_ref()),
        ) {
            direction.validate()?;
        }
        let secondary = self.second_direction.as_ref().map(|d| d.count).unwrap_or(1);
        if count == 0 || secondary == 0 || count.checked_mul(secondary).is_none_or(|n| n > 10000) {
            return Err(CadError::Invalid(
                "Body patterns require 1..10000 total instance positions".into(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for pair in &self.skipped {
            if pair[0] >= count || pair[1] >= secondary || *pair == [0, 0] || !seen.insert(*pair) {
                return Err(CadError::Invalid("Skipped pattern positions must be unique, within both counts, and exclude the seed".into()));
            }
        }
        Ok(secondary)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ScaleOptions {
    /// When uniform, only the X expression controls all three factors.
    pub uniform: bool,
    pub factors: [Expr; 3],
    pub center: PointSource,
    pub keep_original: bool,
}
impl Default for ScaleOptions {
    fn default() -> Self {
        Self {
            uniform: true,
            factors: std::array::from_fn(|_| "1".into()),
            center: PointSource::default(),
            keep_original: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AffineScale {
    pub factors: [f64; 3],
    pub center_mm: [f64; 3],
    /// Row-major affine matrix, in document millimetres.
    pub matrix: [[f64; 4]; 4],
}
impl ScaleOptions {
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut fields = if self.uniform {
            vec![&self.factors[0]]
        } else {
            self.factors.iter().collect()
        };
        fields.extend(self.center.expressions());
        fields
    }
    pub fn resolve(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<AffineScale> {
        let mut factors = [0.; 3];
        for (index, factor) in factors.iter_mut().enumerate() {
            *factor = expression::value(
                doc,
                owner,
                &self.factors[if self.uniform { 0 } else { index }],
                Dimension::Scalar,
            )?;
            if !factor.is_finite() || *factor <= 0. || !(1. / *factor).is_finite() {
                return Err(CadError::Invalid(
                    "Scale factors must be positive finite quantities with finite reciprocals; use Mirror for reflection".into(),
                ));
            }
        }
        let determinant = factors.iter().product::<f64>();
        if !determinant.is_finite() || determinant <= 0. || !(1. / determinant).is_finite() {
            return Err(CadError::Invalid(
                "Scale factors exceed the numeric range of a reversible affine transform".into(),
            ));
        }
        let center_mm = self.center.resolve(doc, owner, resolver)?;
        let mut matrix = [[0.; 4]; 4];
        for index in 0..3 {
            matrix[index][index] = factors[index];
            matrix[index][3] = center_mm[index] * (1. - factors[index]);
        }
        matrix[3][3] = 1.;
        if !center_mm.iter().all(|x| x.is_finite())
            || !matrix.iter().flatten().all(|x| x.is_finite())
        {
            return Err(CadError::Invalid(
                "Scale center or translation exceeds the finite document coordinate range".into(),
            ));
        }
        Ok(AffineScale {
            factors,
            center_mm,
            matrix,
        })
    }
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    options: &ScaleOptions,
    state: &str,
) -> Vec<SemanticElement> {
    let count = if options.uniform { 1 } else { 3 };
    let mut out = Vec::new();
    for index in 0..count {
        let expression = &options.factors[index];
        out.push(SemanticElement {
            id: format!("{node}::scale::factor_{index}"),
            role: "feature-field".into(),
            label: if options.uniform {
                "Scale Factor".into()
            } else {
                format!("{} Factor", ["X", "Y", "Z"][index])
            },
            value: Some(expression.0.clone()),
            state: state.into(),
            commands: vec!["feature.edit".into()],
            relationships: vec![node.into()],
            data: serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"field":format!("factor_{index}"),"type":"quantity","dimension":Dimension::Scalar,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,Dimension::Scalar).ok(),"editable":true}),
        });
    }
    out.push(SemanticElement {
        id: format!("{node}::scale::options"),
        role: "feature-options".into(),
        label: "Scale Options".into(),
        value: None,
        state: state.into(),
        commands: vec!["feature.edit".into()],
        relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"options":options,"center_sources":["coordinates","datum","sketch_point","plane_origin","axis_origin"],"geometry":"positive_uniform_or_nonuniform_affine_brep","axes":"document_xyz","editable":true}),
    });
    out
}

pub fn pattern_semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &crate::Operation,
    state: &str,
) -> Vec<SemanticElement> {
    let crate::Operation::Pattern {
        count,
        step,
        angle,
        options,
        ..
    } = operation
    else {
        return vec![];
    };
    let mut quantities: Vec<(&str, String, &Expr, Dimension)> = step
        .iter()
        .enumerate()
        .map(|(i, e)| {
            (
                "step",
                format!("{} Spacing", ["X", "Y", "Z"][i]),
                e,
                Dimension::Length,
            )
        })
        .collect();
    quantities.push((
        "angle",
        if options.angle_is_total {
            "Total Angle".into()
        } else {
            "Rotation per Instance".into()
        },
        angle,
        Dimension::Angle,
    ));
    if let Some(second) = &options.second_direction {
        quantities.extend(second.step.iter().enumerate().map(|(i, e)| {
            (
                "second_step",
                format!("Direction 2 · {} Spacing", ["X", "Y", "Z"][i]),
                e,
                Dimension::Length,
            )
        }));
    }
    let mut out: Vec<_> = quantities.into_iter().enumerate().map(|(index,(field,label,expression,dimension))|SemanticElement {
        id:format!("{node}::pattern::quantity_{index}"),role:"feature-field".into(),label,value:Some(expression.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],
        data:serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"field":field,"index":index,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect();
    for (slot, direction) in options.direction.iter().map(|d| (0, d)).chain(
        options
            .second_direction
            .iter()
            .filter_map(|d| d.direction.as_ref())
            .map(|d| (1, d)),
    ) {
        let dependencies = direction.dependencies();
        out.push(SemanticElement {id:format!("{node}::pattern::direction_{slot}"),role:"feature-reference".into(),label:format!("Direction {} reference",slot+1),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:std::iter::once(node.into()).chain(dependencies.iter().map(|r|r.node.clone())).collect(),data:serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"slot":slot,"source":direction,"reference_kind":"straight_direction","occurrence_path":direction.occurrence,"editable":true})});
        out.push(SemanticElement {id:format!("{node}::pattern::direction_{slot}_spacing"),role:"feature-field".into(),label:format!("Direction {} spacing",slot+1),value:Some(direction.spacing.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"field":if slot==0{"options/direction/spacing"}else{"options/second_direction/direction/spacing"},"type":"quantity","dimension":Dimension::Length,"expression":direction.spacing,"evaluated":expression::value(doc,owner,&direction.spacing,Dimension::Length).ok(),"editable":true})});
    }
    out.push(SemanticElement {id:format!("{node}::pattern::options"),role:"feature-options".into(),label:"Body Pattern Options".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"count":count,"options":options,"scope":"complete_body_set","skipped_indices":"zero_based_direction_1_direction_2","full_circle_spacing":"total_angle_divided_by_count","partial_arc_spacing":"total_angle_divided_by_count_minus_one","editable":true})});
    out
}
