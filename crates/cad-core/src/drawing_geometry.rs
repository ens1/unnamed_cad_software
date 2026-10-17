//! Disposable native geometry for drawing visibility. These values describe
//! evaluated carriers in mesh-local millimetres, never authored design intent.
//! Native UV coordinates are retained, including angular analytic parameters.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeGeometry {
    pub version: u32,
    /// Index of the containing MeshData.faces carrier. Mesh-local and disposable.
    pub face: usize,
    /// Exact native face-local edge locus, in absolute edge direction. This is
    /// not an intersection search guide and need not share its parameter speed.
    pub curve: Curve2,
    pub range: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceGeometry {
    pub version: u32,
    pub carrier: Carrier,
    /// Closed enclosing ranges in the carrier's own u and v parameters.
    pub domain: [[f64; 2]; 2],
    /// Absolute face boundary loops, with each curve directed along its wire.
    /// Hole membership follows parity over all loops, independently of winding.
    pub loops: Vec<Vec<Curve2>>,
    /// The adapter proved cyclic closure in this UV chart. Native periodic
    /// bands may include exact virtual chart seams, never drawing source edges.
    pub uv_closed: bool,
    /// Optional derived UV chart. Original native loops above remain intact.
    /// Only proven periodic bands receive virtual seam connectors here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_chart_loops: Option<Vec<Vec<Curve2>>>,
    /// Outward physical bound for the native binary64 angular period versus
    /// the exact trigonometric period at a derived chart seam, in millimetres.
    #[serde(default)]
    pub uv_chart_error_mm: f64,
    /// Native face orientation relative to the stored carrier.
    pub forward: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Carrier {
    Plane {
        origin: [f64; 3],
        /// Original defining points; interval consumers subtract the origin.
        u_point: [f64; 3],
        v_point: [f64; 3],
    },
    Rational {
        surface: RationalSurface,
    },
    /// S = transform(center + r(sin(u)cos(v), sin(u)sin(v), cos(u))).
    Sphere {
        center: [f64; 3],
        radius: f64,
        transform: [[f64; 4]; 4],
        swap_uv: bool,
    },
    /// S = transform(center + ((R+r cos(v))cos(u), (R+r cos(v))sin(u), r sin(v))).
    Torus {
        center: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
        transform: [[f64; 4]; 4],
        swap_uv: bool,
    },
    /// S = transform(origin + Rodrigues(profile(u)-origin, axis, v)).
    /// The axis is the original stored native unit axis; do not renormalize it.
    Revolution {
        profile: Curve3,
        origin: [f64; 3],
        axis: [f64; 3],
        transform: [[f64; 4]; 4],
        swap_uv: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RationalSurface {
    pub degree: [usize; 2],
    pub knots: [Vec<f64>; 2],
    /// u-major tensor-product controls [x*w, y*w, z*w, w], positive weights.
    pub control_points: Vec<Vec<[f64; 4]>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve2 {
    Rational {
        degree: usize,
        knots: Vec<f64>,
        control_points: Vec<[f64; 3]>,
        reversed: bool,
    },
    /// Native conic UV, retaining angular/hyperbolic parameter semantics.
    Conic {
        conic: Conic,
        transform: [[f64; 3]; 3],
        range: [f64; 2],
        reversed: bool,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conic {
    /// (cos(t), sin(t)).
    Ellipse,
    /// (cosh(t), sinh(t)).
    Hyperbola,
    /// (t*t, 2*t).
    Parabola,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve3 {
    /// Native unbounded line: P(t) = start + t*(end-start).
    Line { start: [f64; 3], end: [f64; 3] },
    Rational {
        degree: usize,
        knots: Vec<f64>,
        control_points: Vec<[f64; 4]>,
    },
}
