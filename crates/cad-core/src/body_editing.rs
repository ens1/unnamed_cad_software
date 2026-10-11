//! Versioned body-editing intent, independent of kernel and renderer objects.
use crate::{
    CadError, Dimension, Document, Expr, Result, expression,
    presentation::SemanticElement,
    reference_geometry::{PointSource, Resolver},
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LinearPatternDirection {
    pub count: usize,
    pub step: [Expr; 3],
}
impl Default for LinearPatternDirection {
    fn default() -> Self {
        Self {
            count: 2,
            step: ["0 mm".into(), "30 mm".into(), "0 mm".into()],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PatternOptions {
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
        let mut out: Vec<_> = self
            .second_direction
            .iter()
            .flat_map(|direction| direction.step.iter())
            .collect();
        if !self.rotate_instances {
            out.extend(self.seed.expressions());
        }
        out
    }
    pub fn validate(&self, count: usize) -> Result<usize> {
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
    out.push(SemanticElement {id:format!("{node}::pattern::options"),role:"feature-options".into(),label:"Body Pattern Options".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.body-editing","version":CONTRACT_VERSION,"count":count,"options":options,"scope":"complete_body_set","skipped_indices":"zero_based_direction_1_direction_2","full_circle_spacing":"total_angle_divided_by_count","partial_arc_spacing":"total_angle_divided_by_count_minus_one","editable":true})});
    out
}
