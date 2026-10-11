//! Versioned application-owned section, connector and frame intent.
use crate::{Expr, Id, NodeRef, ThinExtrude, reference_geometry::AxisSource};
use serde::{Deserialize, Serialize};
pub const CONTRACT_VERSION: u32 = 1;
fn zero() -> Expr {
    "0".into()
}
fn one() -> Expr {
    "1".into()
}
fn angle_zero() -> Expr {
    "0 deg".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    pub profile: NodeRef,
    #[serde(default)]
    pub region: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_member: Option<Id>,
    pub contour: usize,
    pub vertex: Id,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum End {
    #[default]
    Default,
    Normal {
        influence: Expr,
    },
    Direction {
        axis: Box<AxisSource>,
        influence: Expr,
        #[serde(default)]
        reverse: bool,
    },
}
impl End {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Direction { axis, .. } => axis.dependencies(),
            _ => vec![],
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Normal { influence } | Self::Direction { influence, .. } => vec![influence],
            Self::Default => vec![],
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LoftOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contours: Vec<crate::contours::Selection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<NodeRef>,
    #[serde(default)]
    pub connectors: Vec<Connector>,
    #[serde(default)]
    pub start: End,
    #[serde(default)]
    pub end: End,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinExtrude>,
}
pub fn loft_default(value: &LoftOptions) -> bool {
    value == &LoftOptions::default()
}
impl LoftOptions {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        self.start
            .dependencies()
            .into_iter()
            .chain(self.end.dependencies())
            .chain(
                self.connectors
                    .iter()
                    .map(|connector| connector.profile.clone()),
            )
            .chain(
                self.contours
                    .iter()
                    .map(|selection| selection.profile.clone()),
            )
            .chain(self.scope.iter().cloned())
            .collect()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        self.start
            .expressions()
            .into_iter()
            .chain(self.end.expressions())
            .chain(self.thin.iter().flat_map(|thin| {
                std::iter::once(&thin.thickness).chain(thin.second_thickness.iter())
            }))
            .collect()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Fixed,
    FollowTangent,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    KeepSource,
    AtPathStart,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contours: Vec<crate::contours::Selection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<NodeRef>,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_member: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_start_vertex: Option<Id>,
    #[serde(default = "zero")]
    pub start: Expr,
    #[serde(default = "one")]
    pub end: Expr,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default = "angle_zero")]
    pub roll: Expr,
    /// Exact rational half-angle progression in each retained path Bézier span.
    /// This is a native angle law, not an approximation of linear-rate twist.
    #[serde(default = "angle_zero")]
    pub twist: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinExtrude>,
}
impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            contours: vec![],
            scope: vec![],
            orientation: Orientation::Fixed,
            placement: Placement::KeepSource,
            path_member: None,
            path_start_vertex: None,
            start: zero(),
            end: one(),
            reverse: false,
            roll: angle_zero(),
            twist: angle_zero(),
            thin: None,
        }
    }
}
pub fn sweep_default(value: &SweepOptions) -> bool {
    value == &SweepOptions::default()
}
impl SweepOptions {
    pub fn expressions(&self) -> Vec<&Expr> {
        std::iter::once(&self.start)
            .chain(std::iter::once(&self.end))
            .chain(std::iter::once(&self.roll))
            .chain(std::iter::once(&self.twist))
            .chain(self.thin.iter().flat_map(|thin| {
                std::iter::once(&thin.thickness).chain(thin.second_thickness.iter())
            }))
            .collect()
    }
}
