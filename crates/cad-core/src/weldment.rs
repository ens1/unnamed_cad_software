//! Versioned sketch section/path member groups and associative fabrication intent.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, reference_geometry::PlaneSource,
};
use serde::{Deserialize, Serialize};
pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    Miter,
    Butt,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    #[default]
    Finish,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemberRef {
    pub group: NodeRef,
    pub member: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WeldmentOperation {
    Members {
        #[serde(default)]
        body: Option<NodeRef>,
        section: NodeRef,
        path: NodeRef,
        #[serde(default)]
        members: Vec<Id>,
        roll: Expr,
        #[serde(default)]
        anchor: Option<Id>,
        start_offset: Expr,
        end_offset: Expr,
        #[serde(default)]
        corner: Corner,
    },
    Trim {
        body: NodeRef,
        #[serde(default)]
        members: Vec<MemberRef>,
        plane: PlaneSource,
        offset: Expr,
        #[serde(default)]
        end: End,
    },
    EndCap {
        body: NodeRef,
        member: MemberRef,
        #[serde(default)]
        end: End,
        thickness: Expr,
        offset: Expr,
    },
    Gusset {
        #[serde(default)]
        body: Option<NodeRef>,
        profile: NodeRef,
        thickness: Expr,
        #[serde(default)]
        side: crate::sheet_metal::ThicknessSide,
    },
}
impl WeldmentOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Members { .. } => "Structural Member Group",
            Self::Trim { .. } => "Trim / Extend Member",
            Self::EndCap { .. } => "End Cap",
            Self::Gusset { .. } => "Sketch Gusset",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut refs = self.consumed_outputs();
        match self {
            Self::Members { section, path, .. } => {
                refs.push(section.clone());
                refs.push(path.clone());
            }
            Self::Trim { plane, .. } => refs.extend(plane.dependencies()),
            Self::Gusset { profile, .. } => refs.push(profile.clone()),
            Self::EndCap { .. } => {}
        }
        refs
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        match self {
            Self::Members { body, .. } | Self::Gusset { body, .. } => {
                body.iter().cloned().collect()
            }
            Self::Trim { body, .. } | Self::EndCap { body, .. } => vec![body.clone()],
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Members {
                roll,
                start_offset,
                end_offset,
                ..
            } => vec![roll, start_offset, end_offset],
            Self::Trim { offset, .. } => vec![offset],
            Self::EndCap {
                thickness, offset, ..
            } => vec![thickness, offset],
            Self::Gusset { thickness, .. } => vec![thickness],
        }
    }
}
#[derive(Clone, Debug)]
pub struct Group {
    pub source: NodeRef,
    pub section: NodeRef,
    pub path: NodeRef,
    pub members: Vec<Id>,
    pub roll_rad: f64,
    pub anchor: Option<Id>,
    pub start_offset_mm: f64,
    pub end_offset_mm: f64,
    pub corner: Corner,
}
#[derive(Clone, Debug)]
pub struct Trim {
    pub source: NodeRef,
    pub groups: Vec<NodeRef>,
    pub members: Vec<MemberRef>,
    pub plane: PlaneSource,
    pub offset_mm: f64,
    pub end: End,
}
#[derive(Clone, Debug)]
pub struct Cap {
    pub source: NodeRef,
    pub member: MemberRef,
    pub end: End,
    pub thickness_mm: f64,
    pub offset_mm: f64,
}
#[derive(Clone, Debug)]
pub struct Plate {
    pub source: NodeRef,
    pub profile: NodeRef,
    pub thickness_mm: f64,
    pub side: crate::sheet_metal::ThicknessSide,
}
#[derive(Clone, Debug, Default)]
pub struct Stock {
    pub groups: Vec<Group>,
    pub trims: Vec<Trim>,
    pub caps: Vec<Cap>,
    pub plates: Vec<Plate>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Weldment: {message}"))
}
pub fn stock(doc: &Document, reference: &NodeRef) -> Result<Stock> {
    fn resolve(
        doc: &Document,
        reference: &NodeRef,
        stack: &mut std::collections::BTreeSet<NodeRef>,
    ) -> Result<Stock> {
        if !stack.insert(reference.clone()) {
            return Err(invalid("Member dependency cycle"));
        }
        let feature = doc
            .parts
            .get(&reference.definition)
            .and_then(|part| part.features.iter().find(|f| f.id == reference.node))
            .ok_or_else(|| invalid("Weldment feature is missing"))?;
        let crate::Operation::Weldment { operation } = &feature.operation else {
            return Err(CadError::Unsupported("Native member editing requires sketch-based weldment stock; imported or generic solid recognition is not inferred".into()));
        };
        if feature.suppressed {
            let body = operation
                .consumed_outputs()
                .into_iter()
                .next()
                .ok_or_else(|| {
                    invalid("A suppressed base member group cannot define weldment stock")
                })?;
            let stock = resolve(doc, &body, stack)?;
            stack.remove(reference);
            return Ok(stock);
        }
        let length = |expression: &Expr| {
            crate::expression::value(doc, &reference.definition, expression, Dimension::Length)
        };
        let mut stock = match operation.as_ref() {
            WeldmentOperation::Members { body, .. } | WeldmentOperation::Gusset { body, .. } => {
                body.as_ref()
                    .map(|body| resolve(doc, body, stack))
                    .transpose()?
                    .unwrap_or_default()
            }
            WeldmentOperation::Trim { body, .. } | WeldmentOperation::EndCap { body, .. } => {
                resolve(doc, body, stack)?
            }
        };
        match operation.as_ref() {
            WeldmentOperation::Members {
                section,
                path,
                members,
                roll,
                anchor,
                start_offset,
                end_offset,
                corner,
                ..
            } => {
                if members
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != members.len()
                {
                    return Err(invalid("Member selection contains duplicates"));
                }
                stock.groups.push(Group {
                    source: reference.clone(),
                    section: section.clone(),
                    path: path.clone(),
                    members: members.clone(),
                    roll_rad: crate::expression::value(
                        doc,
                        &reference.definition,
                        roll,
                        Dimension::Angle,
                    )?,
                    anchor: anchor.clone(),
                    start_offset_mm: length(start_offset)?,
                    end_offset_mm: length(end_offset)?,
                    corner: *corner,
                });
            }
            WeldmentOperation::Trim {
                members,
                plane,
                offset,
                end,
                ..
            } => {
                if members
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != members.len()
                    || members.iter().any(|member| {
                        !stock
                            .groups
                            .iter()
                            .any(|group| group.source == member.group)
                    })
                {
                    return Err(invalid(
                        "Trim member references must be unique and belong to inherited stock",
                    ));
                }
                stock.trims.push(Trim {
                    source: reference.clone(),
                    groups: stock
                        .groups
                        .iter()
                        .map(|group| group.source.clone())
                        .collect(),
                    members: members.clone(),
                    plane: plane.clone(),
                    offset_mm: length(offset)?,
                    end: *end,
                });
            }
            WeldmentOperation::EndCap {
                member,
                end,
                thickness,
                offset,
                ..
            } => {
                let thickness = length(thickness)?;
                let offset = length(offset)?;
                if !stock
                    .groups
                    .iter()
                    .any(|group| group.source == member.group)
                {
                    return Err(invalid("End-cap member must belong to inherited stock"));
                }
                if thickness <= 1e-7 || offset < 0. {
                    return Err(invalid(
                        "End caps need positive thickness and nonnegative outward gap",
                    ));
                }
                if stock.caps.iter().any(|cap| {
                    cap.member == *member
                        && cap.end == *end
                        && offset < cap.offset_mm + cap.thickness_mm - 1e-7
                        && cap.offset_mm < offset + thickness - 1e-7
                }) {
                    return Err(invalid("End caps overlap at the same member end"));
                }
                stock.caps.push(Cap {
                    source: reference.clone(),
                    member: member.clone(),
                    end: *end,
                    thickness_mm: thickness,
                    offset_mm: offset,
                });
            }
            WeldmentOperation::Gusset {
                profile,
                thickness,
                side,
                ..
            } => {
                let thickness = length(thickness)?;
                if thickness <= 1e-7 {
                    return Err(invalid("Gusset thickness must be positive"));
                }
                stock.plates.push(Plate {
                    source: reference.clone(),
                    profile: profile.clone(),
                    thickness_mm: thickness,
                    side: *side,
                });
            }
        }
        stack.remove(reference);
        Ok(stock)
    }
    resolve(doc, reference, &mut Default::default())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedMember {
    pub reference: MemberRef,
    pub section: NodeRef,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub centerline_length_mm: f64,
    pub start_cut_angle_rad: f64,
    pub end_cut_angle_rad: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedWeldment {
    pub version: u32,
    pub source: NodeRef,
    pub members: Vec<EvaluatedMember>,
    pub cap_count: usize,
    pub gusset_count: usize,
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &WeldmentOperation,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let quantities: Vec<(&str, &str, &Expr, Dimension)> = match operation {
        WeldmentOperation::Members {
            roll,
            start_offset,
            end_offset,
            ..
        } => vec![
            ("roll", "Section Rotation", roll, Dimension::Angle),
            (
                "start_offset",
                "Start Shortening / Extension",
                start_offset,
                Dimension::Length,
            ),
            (
                "end_offset",
                "Finish Shortening / Extension",
                end_offset,
                Dimension::Length,
            ),
        ],
        WeldmentOperation::Trim { offset, .. } => {
            vec![("offset", "Trim Plane Offset", offset, Dimension::Length)]
        }
        WeldmentOperation::EndCap {
            thickness, offset, ..
        } => vec![
            ("thickness", "Cap Thickness", thickness, Dimension::Length),
            ("offset", "Outward Gap", offset, Dimension::Length),
        ],
        WeldmentOperation::Gusset { thickness, .. } => vec![(
            "thickness",
            "Gusset Thickness",
            thickness,
            Dimension::Length,
        )],
    };
    let mut fields: Vec<_> = quantities.into_iter().map(|(field, label, expression, dimension)| crate::presentation::SemanticElement {
        id: format!("{node}::weldment::{field}"), role: "feature-field".into(), label: label.into(), value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()], relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.weldment","version":CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect();
    fields.push(crate::presentation::SemanticElement { id: format!("{node}::weldment::options"), role: "feature-options".into(), label: operation.name().into(), value: None, state: state.into(), commands: vec!["feature.edit".into(),"weldment.members".into(),"weldment.trim".into(),"weldment.cap".into(),"weldment.gusset".into()], relationships: operation.dependencies().into_iter().map(|source| source.node).collect(),
        data: serde_json::json!({"contract":"tcad.weldment","version":CONTRACT_VERSION,"source":operation,"editable":true,"capabilities":{"section":"one_closed_region_with_optional_holes","paths":"planar_straight_sketch_members_including_construction","placement":["sketch_origin","locating_point","roll"],"groups":"multiple_saved_section_path_groups","corners":["two_incident_member_miter","independent_butt"],"trim":"associative_planar_end_projection","caps":"terminal_or_explicitly_trimmed_end","gussets":"closed_sketch_plate","manufacturing_length":"evaluated_centerline_between_end_planes","members_remain_separate_solids":true}}),
    });
    fields
}
