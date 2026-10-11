//! Versioned sketch-based sheet stock, bend intent, and derived manufacturing data.
use crate::{CadError, Dimension, Document, Expr, Id, NodeRef, Plane, Result};
use serde::{Deserialize, Serialize};
pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Formed,
    Flat,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThicknessSide {
    #[default]
    Positive,
    Negative,
    MidPlane,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SheetOperation {
    Base {
        profile: NodeRef,
        thickness: Expr,
        inside_radius: Expr,
        k_factor: Expr,
        #[serde(default)]
        side: ThicknessSide,
    },
    Tab {
        body: NodeRef,
        profile: NodeRef,
    },
    EdgeFlange {
        body: NodeRef,
        member: Id,
        length: Expr,
        angle: Expr,
        #[serde(default)]
        inside_radius: Option<Expr>,
        #[serde(default)]
        k_factor: Option<Expr>,
    },
    Flatten {
        body: NodeRef,
        #[serde(default)]
        state: State,
    },
}
impl SheetOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Base { .. } => "Base Flange / Tab",
            Self::Tab { .. } => "Sheet Tab",
            Self::EdgeFlange { .. } => "Edge Flange",
            Self::Flatten {
                state: State::Flat, ..
            } => "Flat Pattern",
            Self::Flatten { .. } => "Formed Sheet",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Base { profile, .. } => vec![profile.clone()],
            Self::Tab { body, profile } => vec![body.clone(), profile.clone()],
            Self::EdgeFlange { body, .. } | Self::Flatten { body, .. } => vec![body.clone()],
        }
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        match self {
            Self::Base { .. } => vec![],
            Self::Tab { body, .. } | Self::EdgeFlange { body, .. } | Self::Flatten { body, .. } => {
                vec![body.clone()]
            }
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Base {
                thickness,
                inside_radius,
                k_factor,
                ..
            } => vec![thickness, inside_radius, k_factor],
            Self::EdgeFlange {
                length,
                angle,
                inside_radius,
                k_factor,
                ..
            } => {
                let mut values = vec![length, angle];
                values.extend(inside_radius.iter());
                values.extend(k_factor.iter());
                values
            }
            _ => vec![],
        }
    }
}
#[derive(Clone, Debug)]
pub struct Flange {
    pub source: NodeRef,
    pub member: Id,
    pub length_mm: f64,
    pub angle_rad: f64,
    pub radius_mm: f64,
    pub k_factor: f64,
}
impl Flange {
    pub fn allowance_mm(&self, thickness: f64) -> f64 {
        self.angle_rad.abs() * (self.radius_mm + self.k_factor * thickness)
    }
}
#[derive(Clone, Debug)]
pub struct Stock {
    pub source: NodeRef,
    pub profile: NodeRef,
    pub tabs: Vec<NodeRef>,
    pub thickness_mm: f64,
    pub radius_mm: f64,
    pub k_factor: f64,
    pub side: ThicknessSide,
    pub state: State,
    pub flanges: Vec<Flange>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Sheet metal: {message}"))
}
pub fn stock(doc: &Document, reference: &NodeRef) -> Result<Stock> {
    fn resolve(
        doc: &Document,
        reference: &NodeRef,
        stack: &mut std::collections::BTreeSet<NodeRef>,
    ) -> Result<Stock> {
        if !stack.insert(reference.clone()) {
            return Err(invalid("Stock dependency cycle"));
        }
        let feature = doc
            .parts
            .get(&reference.definition)
            .and_then(|part| part.features.iter().find(|f| f.id == reference.node))
            .ok_or_else(|| invalid("Sheet feature is missing"))?;
        let crate::Operation::Sheet { operation } = &feature.operation else {
            return Err(CadError::Unsupported("Sheet commands require native sketch-based sheet stock; arbitrary solid-to-sheet recognition is not inferred".into()));
        };
        if feature.suppressed {
            let body = match operation.as_ref() {
                SheetOperation::Tab { body, .. }
                | SheetOperation::EdgeFlange { body, .. }
                | SheetOperation::Flatten { body, .. } => body,
                SheetOperation::Base { .. } => {
                    return Err(invalid("A suppressed base cannot define sheet stock"));
                }
            };
            let stock = resolve(doc, body, stack)?;
            stack.remove(reference);
            return Ok(stock);
        }
        let length = |expression: &Expr| {
            crate::expression::value(doc, &reference.definition, expression, Dimension::Length)
        };
        let value = match operation.as_ref() {
            SheetOperation::Base {
                profile,
                thickness,
                inside_radius,
                k_factor,
                side,
            } => {
                let thickness = length(thickness)?;
                let radius = length(inside_radius)?;
                let k_factor = crate::expression::value(
                    doc,
                    &reference.definition,
                    k_factor,
                    Dimension::Scalar,
                )?;
                if thickness <= 1e-7
                    || radius <= 0.
                    || !k_factor.is_finite()
                    || !(0.0..=1.0).contains(&k_factor)
                {
                    return Err(invalid(
                        "Stock needs positive thickness/radius and a finite K factor in [0,1]",
                    ));
                }
                Stock {
                    source: reference.clone(),
                    profile: profile.clone(),
                    tabs: vec![],
                    thickness_mm: thickness,
                    radius_mm: radius,
                    k_factor,
                    side: *side,
                    state: State::Formed,
                    flanges: vec![],
                }
            }
            SheetOperation::Tab { body, profile } => {
                let mut stock = resolve(doc, body, stack)?;
                if !stock.flanges.is_empty() {
                    return Err(CadError::Unsupported("Add sheet tabs before edge flanges; trimming tabs through developed bends requires bend-aware Boolean propagation".into()));
                }
                stock.tabs.push(profile.clone());
                stock
            }
            SheetOperation::EdgeFlange {
                body,
                member,
                length: extension,
                angle,
                inside_radius,
                k_factor,
            } => {
                let mut stock = resolve(doc, body, stack)?;
                let extension = length(extension)?;
                let angle =
                    crate::expression::value(doc, &reference.definition, angle, Dimension::Angle)?;
                let radius = inside_radius
                    .as_ref()
                    .map(length)
                    .transpose()?
                    .unwrap_or(stock.radius_mm);
                let k = k_factor
                    .as_ref()
                    .map(|expression| {
                        crate::expression::value(
                            doc,
                            &reference.definition,
                            expression,
                            Dimension::Scalar,
                        )
                    })
                    .transpose()?
                    .unwrap_or(stock.k_factor);
                if extension <= 1e-7
                    || angle.abs() < 1e-6
                    || angle.abs() > std::f64::consts::FRAC_PI_2 + 1e-8
                    || radius <= 0.
                    || !k.is_finite()
                    || !(0.0..=1.0).contains(&k)
                {
                    return Err(invalid(
                        "Edge flanges require positive tangent length/radius, signed angles through 90 degrees, and K factor in [0,1]",
                    ));
                }
                if stock.flanges.iter().any(|f| f.member == *member) {
                    return Err(invalid("A base sketch member already has an edge flange"));
                }
                stock.flanges.push(Flange {
                    source: reference.clone(),
                    member: member.clone(),
                    length_mm: extension,
                    angle_rad: angle,
                    radius_mm: radius,
                    k_factor: k,
                });
                stock
            }
            SheetOperation::Flatten { body, state } => {
                let mut stock = resolve(doc, body, stack)?;
                stock.state = *state;
                stock
            }
        };
        stack.remove(reference);
        Ok(value)
    }
    resolve(doc, reference, &mut Default::default())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BendLine {
    pub feature: NodeRef,
    pub member: Id,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub angle_rad: f64,
    pub inside_radius_mm: f64,
    pub allowance_mm: f64,
    pub k_factor: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedSheet {
    pub version: u32,
    pub source: NodeRef,
    pub base_source: NodeRef,
    pub profile: NodeRef,
    pub state: State,
    pub plane: Plane,
    pub thickness_mm: f64,
    pub bends: Vec<BendLine>,
    pub flat_contours: Vec<crate::sketch_edit::Curve2>,
    pub flat_export_error: Option<String>,
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &SheetOperation,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let quantities: Vec<(&str, &str, &Expr, Dimension)> = match operation {
        SheetOperation::Base {
            thickness,
            inside_radius,
            k_factor,
            ..
        } => vec![
            ("thickness", "Thickness", thickness, Dimension::Length),
            (
                "inside_radius",
                "Default Inside Radius",
                inside_radius,
                Dimension::Length,
            ),
            ("k_factor", "K Factor", k_factor, Dimension::Scalar),
        ],
        SheetOperation::EdgeFlange {
            length,
            angle,
            inside_radius,
            k_factor,
            ..
        } => {
            let mut fields = vec![
                (
                    "length",
                    "Straight Tangent Length",
                    length,
                    Dimension::Length,
                ),
                ("angle", "Bend Angle", angle, Dimension::Angle),
            ];
            fields.extend(
                inside_radius
                    .iter()
                    .map(|value| ("inside_radius", "Inside Radius", value, Dimension::Length)),
            );
            fields.extend(
                k_factor
                    .iter()
                    .map(|value| ("k_factor", "K Factor", value, Dimension::Scalar)),
            );
            fields
        }
        _ => vec![],
    };
    let mut fields: Vec<_> = quantities.into_iter().map(|(field, label, expression, dimension)| crate::presentation::SemanticElement {
        id: format!("{node}::sheet::{field}"), role: "feature-field".into(), label: label.into(), value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()], relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.sheet_metal","version":CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect();
    fields.push(crate::presentation::SemanticElement { id: format!("{node}::sheet::options"), role: "feature-options".into(), label: operation.name().into(), value: None, state: state.into(), commands: vec!["feature.edit".into(),"sheet.flange".into(),"sheet.flat".into(),"sheet.formed".into()], relationships: operation.dependencies().into_iter().map(|source| source.node).collect(),
        data: serde_json::json!({"contract":"tcad.sheet_metal","version":CONTRACT_VERSION,"source":operation,"editable":true,"capabilities":{"base":"closed_sketch_regions_with_holes","thickness_direction":["positive","negative","mid_plane"],"tabs":"coplanar_before_bends","flanges":"complete_supporting_straight_base_edges","signed_angle_range_degrees":[-90,90],"allowance":"abs(angle)*(inside_radius+K*thickness)","flat_geometry":"exact_brep","relief":false,"imported_body_recognition":false}}),
    });
    fields
}
/// One current blank per distinct placed part/configuration, excluding orphan definitions.
pub fn export_sheets(
    doc: &Document,
    evaluation: &crate::protocol::Evaluation,
) -> Result<std::collections::BTreeMap<String, EvaluatedSheet>> {
    let mut sheets = std::collections::BTreeMap::new();
    for instance in crate::configuration::placed_parts(doc, Some(&evaluation.assemblies))? {
        let part = crate::configuration::effective_part(
            doc,
            &instance.definition,
            instance.configuration.as_deref(),
        )?;
        for feature in part.features.iter().filter(|feature| {
            !feature.suppressed
                && part.outputs.contains(&feature.id)
                && matches!(feature.operation, crate::Operation::Sheet { .. })
        }) {
            let key = format!("{}::{}", instance.mesh_key, feature.id);
            let sheet = evaluation.sheets.get(&key).ok_or_else(|| {
                invalid(&format!(
                    "Current flat-pattern data is unavailable for {} / {}",
                    part.name, feature.name
                ))
            })?;
            sheets.entry(key).or_insert_with(|| sheet.clone());
        }
    }
    Ok(sheets)
}
pub fn dxf(sheets: &std::collections::BTreeMap<String, EvaluatedSheet>) -> Result<String> {
    if sheets.is_empty() {
        return Err(invalid(
            "No evaluated native sheet flat patterns are available",
        ));
    }
    let mut out="0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n".to_string();
    let mut offset: f64 = 0.;
    for (key, sheet) in sheets {
        if let Some(error) = &sheet.flat_export_error {
            return Err(CadError::Unsupported(error.clone()));
        }
        let min = sheet
            .flat_contours
            .iter()
            .flat_map(|c| c.control_points.iter())
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min);
        let max = sheet
            .flat_contours
            .iter()
            .flat_map(|c| c.control_points.iter())
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        if !min.is_finite() || !max.is_finite() || !offset.is_finite() {
            return Err(invalid(
                "Flat pattern has no finite outline or sheet placement",
            ));
        }
        let dx = offset - min;
        if !dx.is_finite() {
            return Err(invalid(
                "Flat-pattern layout exceeds the finite coordinate range",
            ));
        }
        let layer = key
            .chars()
            .take(220)
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        for curve in &sheet.flat_contours {
            out.push_str(&format!("0\nSPLINE\n8\nflat-{layer}\n70\n12\n71\n{}\n72\n{}\n73\n{}\n74\n0\n210\n0\n220\n0\n230\n1\n",curve.degree,curve.knots.len(),curve.control_points.len()));
            for knot in &curve.knots {
                out.push_str(&format!("40\n{knot}\n"));
            }
            for weight in &curve.weights {
                out.push_str(&format!("41\n{weight}\n"));
            }
            for p in &curve.control_points {
                if !(p[0] + dx).is_finite() || !p[1].is_finite() {
                    return Err(invalid(
                        "Flat-pattern control points exceed the finite coordinate range",
                    ));
                }
                out.push_str(&format!("10\n{}\n20\n{}\n30\n0\n", p[0] + dx, p[1]));
            }
        }
        for bend in &sheet.bends {
            if ![
                bend.start[0] + dx,
                bend.start[1],
                bend.end[0] + dx,
                bend.end[1],
            ]
            .iter()
            .all(|value| value.is_finite())
            {
                return Err(invalid(
                    "Flat-pattern bend lines exceed the finite coordinate range",
                ));
            }
            out.push_str(&format!(
                "0\nLINE\n8\nbend-{layer}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
                bend.start[0] + dx,
                bend.start[1],
                bend.end[0] + dx,
                bend.end[1]
            ));
        }
        offset += max - min + 20.;
        if !offset.is_finite() {
            return Err(invalid(
                "Flat-pattern layout exceeds the finite coordinate range",
            ));
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(out)
}
