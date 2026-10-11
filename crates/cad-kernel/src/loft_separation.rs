//! Bounded separation of complete native ruled loft skins. Positive rational
//! Bernstein hulls, with outward arithmetic through de Casteljau refinement,
//! prove disjointness; polygonal display samples are never proof inputs.
use crate::native_bounds::{Interval, IntervalPoint, dot_intervals};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;

const MAX_WORK: usize = 24_000_000;
const MAX_PAIRS: usize = 131_072;
const MAX_CELLS: usize = 65_536;
const MAX_DEPTH: usize = 24;
type Homogeneous = [Interval; 4];

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Loft separation: {message}"))
}
fn charge(work: &mut usize, amount: usize) -> Result<()> {
    *work = work
        .checked_add(amount)
        .ok_or_else(|| unsupported("proof work overflowed"))?;
    if *work > MAX_WORK {
        return Err(unsupported(
            "the complete operation exceeds its bounded native separation work",
        ));
    }
    Ok(())
}
fn physical(h: &Homogeneous) -> Result<IntervalPoint> {
    if h[3].low <= 0. {
        return Err(unsupported("positive finite rational weights are required"));
    }
    Ok([h[0].div(h[3])?, h[1].div(h[3])?, h[2].div(h[3])?])
}
fn difference(a: &IntervalPoint, b: &IntervalPoint) -> Result<IntervalPoint> {
    Ok([a[0].sub(b[0])?, a[1].sub(b[1])?, a[2].sub(b[2])?])
}
fn exact(v: Vector3) -> Result<IntervalPoint> {
    Ok([
        Interval::exact(v.x)?,
        Interval::exact(v.y)?,
        Interval::exact(v.z)?,
    ])
}
fn cross(a: &IntervalPoint, b: &IntervalPoint) -> Result<IntervalPoint> {
    Ok([
        a[1].mul(b[2])?.sub(a[2].mul(b[1])?)?,
        a[2].mul(b[0])?.sub(a[0].mul(b[2])?)?,
        a[0].mul(b[1])?.sub(a[1].mul(b[0])?)?,
    ])
}
fn middle(p: &IntervalPoint) -> Vector3 {
    Vector3::new(
        (p[0].low + p[0].high) * 0.5,
        (p[1].low + p[1].high) * 0.5,
        (p[2].low + p[2].high) * 0.5,
    )
}

fn exact_product_equal(a: f64, b: f64, c: f64, d: f64) -> bool {
    let p = a * b;
    let q = c * d;
    let safe = |x: f64, y: f64, product: f64| {
        product.is_finite() && (x == 0. || y == 0. || product.abs() >= 1e-250)
    };
    safe(a, b, p) && safe(c, d, q) && p == q && a.mul_add(b, -p) == c.mul_add(d, -q)
}
fn same_projective_point(a: &Homogeneous, b: &Homogeneous) -> bool {
    a.iter().chain(b).all(|value| value.low == value.high)
        && (0..3).all(|i| exact_product_equal(a[i].low, b[3].low, b[i].low, a[3].low))
}

pub(super) fn same_projective_coordinate(a: Vector4, b: Vector4, axis: usize) -> bool {
    exact_product_equal(a[axis], b.w, b[axis], a.w)
}

#[derive(Clone)]
struct Cell {
    net: Vec<Vec<Homogeneous>>,
    points: Vec<IntervalPoint>,
    bounds: IntervalPoint,
}
impl Cell {
    fn new(net: Vec<Vec<Homogeneous>>, work: &mut usize) -> Result<Self> {
        charge(work, net.len() * net[0].len() * 24)?;
        let points = net
            .iter()
            .flatten()
            .map(physical)
            .collect::<Result<Vec<_>>>()?;
        let mut bounds = points[0];
        for point in &points {
            for i in 0..3 {
                bounds[i].low = bounds[i].low.min(point[i].low);
                bounds[i].high = bounds[i].high.max(point[i].high);
            }
        }
        Ok(Self {
            net,
            points,
            bounds,
        })
    }
    fn from_face(face: &Face, work: &mut usize) -> Result<Self> {
        let Surface::NurbsSurface(surface) = face.surface() else {
            return Err(unsupported(
                "the skin must retain its native rational carrier",
            ));
        };
        let net = surface.control_points();
        let degree = surface.knot_vector_u().len() - net.len() - 1;
        if net.len() != degree + 1
            || net.len() > 33
            || net.iter().any(|row| row.len() != 2)
            || surface.knot_vector_v().len() != 4
        {
            return Err(unsupported(
                "separation requires one native ruled Bernstein chart per face",
            ));
        }
        Self::new(
            net.iter()
                .map(|row| {
                    row.iter()
                        .map(|h| {
                            Ok([
                                Interval::exact(h.x)?,
                                Interval::exact(h.y)?,
                                Interval::exact(h.z)?,
                                Interval::exact(h.w)?,
                            ])
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
            work,
        )
    }
    fn extent(&self) -> f64 {
        self.bounds
            .iter()
            .map(|a| a.high - a.low)
            .fold(0., f64::max)
    }
    fn split(&self, u: bool, work: &mut usize) -> Result<(Self, Self)> {
        let mut left = self.net.clone();
        let mut right = self.net.clone();
        let degree = if u {
            self.net.len() - 1
        } else {
            self.net[0].len() - 1
        };
        let count = if u { self.net[0].len() } else { self.net.len() };
        let half = Interval::exact(0.5)?;
        charge(work, (degree + 1) * (degree + 1) * count * 8)?;
        for other in 0..count {
            let mut controls = (0..=degree)
                .map(|i| {
                    if u {
                        self.net[i][other]
                    } else {
                        self.net[other][i]
                    }
                })
                .collect::<Vec<_>>();
            for level in 0..=degree {
                if u {
                    left[level][other] = controls[0];
                    right[degree - level][other] = controls[degree - level];
                } else {
                    left[other][level] = controls[0];
                    right[other][degree - level] = controls[degree - level];
                }
                for i in 0..degree - level {
                    let next = controls[i + 1];
                    for (value, next) in controls[i].iter_mut().zip(next) {
                        *value = value.mul(half)?.add(next.mul(half)?)?;
                    }
                }
            }
        }
        Ok((Self::new(left, work)?, Self::new(right, work)?))
    }
    fn split_direction(&self) -> bool {
        let p = self.net.len() - 1;
        let q = self.net[0].len() - 1;
        let point = |i: usize, j: usize| middle(&self.points[i * (q + 1) + j]);
        let along_u = (0..=q)
            .map(|j| (point(p, j) - point(0, j)).magnitude2())
            .fold(0., f64::max);
        let along_v = (0..=p)
            .map(|i| (point(i, q) - point(i, 0)).magnitude2())
            .fold(0., f64::max);
        along_u >= along_v
    }
}

fn boxes_separate(a: &Cell, b: &Cell) -> bool {
    (0..3).any(|i| a.bounds[i].high < b.bounds[i].low || b.bounds[i].high < a.bounds[i].low)
}
fn projection(cell: &Cell, axis: &IntervalPoint, work: &mut usize) -> Result<Interval> {
    charge(work, cell.points.len() * 12)?;
    let mut result = dot_intervals(&cell.points[0], axis)?;
    for point in cell.points.iter().skip(1) {
        let p = dot_intervals(point, axis)?;
        result.low = result.low.min(p.low);
        result.high = result.high.max(p.high);
    }
    Ok(result)
}
fn hulls_separate(a: &Cell, b: &Cell, work: &mut usize) -> Result<bool> {
    if boxes_separate(a, b) {
        return Ok(true);
    }
    let ac = middle(&a.bounds);
    let bc = middle(&b.bounds);
    let mut axes = vec![bc - ac];
    for cell in [a, b] {
        let last = cell.net.len() - 1;
        let x = middle(&cell.points[2 * last]) - middle(&cell.points[0]);
        let y = middle(&cell.points[1]) - middle(&cell.points[0]);
        axes.extend([x, y, x.cross(y)]);
    }
    for axis in axes {
        let length = axis.magnitude();
        if !length.is_finite() || length <= 1e-12 {
            continue;
        }
        let axis = exact(axis / length)?;
        let x = projection(a, &axis, work)?;
        let y = projection(b, &axis, work)?;
        if x.high < y.low || y.high < x.low {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Adjacent faces are exempt only at their shared native connector. A plane
/// through that line must put every other rational control strictly on its
/// own side. The omitted endpoint row is the exact Bernstein factor u or 1-u,
/// so this proves separation up to the common boundary, not an epsilon gap.
fn connector_separates(a: &Cell, b: &Cell, ends: (bool, bool), work: &mut usize) -> Result<bool> {
    let ia = if ends.0 { a.net.len() - 1 } else { 0 };
    let ib = if ends.1 { b.net.len() - 1 } else { 0 };
    if !(0..2).all(|j| same_projective_point(&a.net[ia][j], &b.net[ib][j])) {
        // A shared EdgeId only promises tolerance membership. Omitting both
        // rows requires actual projective equality, or two different planes
        // could falsely hide an overlap near that seam.
        return Ok(false);
    }
    let e0 = physical(&a.net[ia][0])?;
    let e1 = physical(&a.net[ia][1])?;
    let line = difference(&e1, &e0)?;
    let direction = middle(&line);
    let length = direction.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Ok(false);
    }
    let unit = direction / length;
    let inside_a = if ends.0 { ia - 1 } else { 1 };
    let inside_b = if ends.1 { ib - 1 } else { 1 };
    let mut directions = Vec::new();
    for j in 0..2 {
        let da = middle(&difference(
            &physical(&a.net[inside_a][j])?,
            &physical(&a.net[ia][j])?,
        )?);
        let db = middle(&difference(
            &physical(&b.net[inside_b][j])?,
            &physical(&b.net[ib][j])?,
        )?);
        directions.extend([db - da, db, -da]);
    }
    directions.push(directions[0] + directions[3]);
    for candidate in directions {
        // Computing a candidate can round freely. The determinant functional
        // is then enclosed from exact candidate coefficients and the native
        // line itself, so perpendicularity is an algebraic identity.
        let auxiliary = candidate.cross(unit);
        if auxiliary.magnitude() <= 1e-12 {
            continue;
        }
        let axis = cross(&line, &exact(auxiliary.normalize())?)?;
        let mut separated = true;
        for (cell, endpoint, sign) in [(a, ia, -1.), (b, ib, 1.)] {
            for (i, row) in cell.net.iter().enumerate() {
                if i == endpoint {
                    continue;
                }
                for (j, h) in row.iter().enumerate() {
                    charge(work, 48)?;
                    let relative = difference(&physical(h)?, &physical(&cell.net[endpoint][j])?)?;
                    let value = dot_intervals(&relative, &axis)?;
                    separated &= if sign < 0. {
                        value.high < 0.
                    } else {
                        value.low > 0.
                    };
                }
            }
        }
        if separated {
            return Ok(true);
        }
    }
    Ok(false)
}

fn shared_connector(a: &Face, b: &Face) -> Result<Option<(bool, bool)>> {
    let wa = a.boundaries();
    let wb = b.boundaries();
    if wa.len() != 1 || wb.len() != 1 || wa[0].len() != 4 || wb[0].len() != 4 {
        return Err(unsupported(
            "a ruled chart lost its native four-sided boundary",
        ));
    }
    let mut shared = None;
    for (i, x) in wa[0].iter().enumerate() {
        for (j, y) in wb[0].iter().enumerate() {
            if x.id() != y.id() {
                continue;
            }
            if ![1, 3].contains(&i) || ![1, 3].contains(&j) || shared.is_some() {
                return Err(unsupported(
                    "loft charts have unexpected shared boundary topology",
                ));
            }
            shared = Some((i == 1, j == 1));
        }
    }
    Ok(shared)
}

fn pair(a: Cell, b: Cell, shared: Option<(bool, bool)>, work: &mut usize) -> Result<()> {
    let mut pending = vec![(a, b, shared, 0usize)];
    let mut visited = 0;
    while let Some((a, b, shared, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_CELLS {
            return Err(unsupported(
                "a native face pair exceeded the separation cell budget",
            ));
        }
        if let Some(ends) = shared {
            if connector_separates(&a, &b, ends, work)? {
                continue;
            }
        } else if hulls_separate(&a, &b, work)? {
            continue;
        }
        if depth >= MAX_DEPTH {
            return Err(unsupported(
                "native loft strips intersect, touch away from a shared connector, or cannot be separated within bounded refinement; this is not a proof that the source is folded",
            ));
        }
        let split_a = a.extent() >= b.extent();
        let u = shared.is_some()
            || if split_a {
                a.split_direction()
            } else {
                b.split_direction()
            };
        if split_a {
            let (left, right) = a.split(u, work)?;
            let left_shared = shared.filter(|ends| !ends.0);
            let right_shared = shared.filter(|ends| ends.0);
            pending.push((right, b.clone(), right_shared, depth + 1));
            pending.push((left, b, left_shared, depth + 1));
        } else {
            let (left, right) = b.split(u, work)?;
            let left_shared = shared.filter(|ends| !ends.1);
            let right_shared = shared.filter(|ends| ends.1);
            pending.push((a.clone(), right, right_shared, depth + 1));
            pending.push((a, left, left_shared, depth + 1));
        }
    }
    Ok(())
}

/// Certify mutual separation of all faces in one section-pair skin. Each face
/// must separately have its complete native injectivity certificate. This
/// does not establish separation from a different section-pair skin.
pub(crate) fn prove_skin_separation(skin: &Shell, work: &mut usize) -> Result<()> {
    if skin.is_empty() || skin.len() > 4096 {
        return Err(unsupported(
            "one skin must contain one through 4096 native charts",
        ));
    }
    let cells = skin
        .iter()
        .map(|face| Cell::from_face(face, work))
        .collect::<Result<Vec<_>>>()?;
    let mut order = (0..cells.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| cells[*a].bounds[0].low.total_cmp(&cells[*b].bounds[0].low));
    let mut pairs = 0;
    for (position, a) in order.iter().copied().enumerate() {
        for b in order.iter().copied().skip(position + 1) {
            if cells[b].bounds[0].low > cells[a].bounds[0].high {
                break;
            }
            charge(work, 8)?;
            if boxes_separate(&cells[a], &cells[b]) {
                continue;
            }
            pairs += 1;
            if pairs > MAX_PAIRS {
                return Err(unsupported(
                    "the native skin exceeds its overlapping-face pair budget",
                ));
            }
            let shared = shared_connector(&skin[a], &skin[b])?;
            pair(cells[a].clone(), cells[b].clone(), shared, work).map_err(
                |error| match error {
                    CadError::Unsupported(message) => CadError::Unsupported(format!(
                        "Native strips {} and {}: {message}",
                        a + 1,
                        b + 1
                    )),
                    other => other,
                },
            )?;
        }
    }
    Ok(())
}
