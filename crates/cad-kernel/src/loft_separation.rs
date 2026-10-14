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
    domain: [f64; 4],
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
            domain: [0., 1., 0., 1.],
        })
    }
    fn from_face(face: &Face, work: &mut usize) -> Result<Self> {
        let Surface::NurbsSurface(surface) = face.surface() else {
            return Err(unsupported(
                "the skin must retain its native rational carrier",
            ));
        };
        Self::from_surface(&surface, work)
    }
    fn from_surface(surface: &NurbsSurface<Vector4>, work: &mut usize) -> Result<Self> {
        let ((u0, u1), (v0, v1)) = surface.range_tuple();
        if ![u0, u1, v0, v1, u1 - u0, v1 - v0]
            .into_iter()
            .all(f64::is_finite)
            || u0 >= u1
            || v0 >= v1
        {
            return Err(unsupported(
                "native chart parameter ranges must be finite and strictly increasing",
            ));
        }
        let net = surface.control_points();
        let degree_u = surface.knot_vector_u().len().checked_sub(net.len() + 1);
        let degree_v = surface
            .knot_vector_v()
            .len()
            .checked_sub(net.first().map_or(0, Vec::len) + 1);
        if !surface.is_clamped()
            || degree_u.is_none_or(|d| !(1..=16).contains(&d) || net.len() != d + 1)
            || degree_v
                .is_none_or(|d| !(1..=16).contains(&d) || net.iter().any(|row| row.len() != d + 1))
        {
            return Err(unsupported(
                "separation requires one positive rational Bernstein chart of degree one through sixteen",
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
        let axis = if u { 0 } else { 2 };
        self.split_at(u, (self.domain[axis] + self.domain[axis + 1]) * 0.5, work)
    }
    fn split_at(&self, u: bool, coordinate: f64, work: &mut usize) -> Result<(Self, Self)> {
        let axis = if u { 0 } else { 2 };
        if coordinate <= self.domain[axis] || coordinate >= self.domain[axis + 1] {
            return Err(unsupported(
                "chart subdivision has no strict native parameter interval",
            ));
        }
        let mut left = self.net.clone();
        let mut right = self.net.clone();
        let degree = if u {
            self.net.len() - 1
        } else {
            self.net[0].len() - 1
        };
        let count = if u { self.net[0].len() } else { self.net.len() };
        let fraction = Interval::exact(coordinate)?
            .sub(Interval::exact(self.domain[axis])?)?
            .div(
                Interval::exact(self.domain[axis + 1])?.sub(Interval::exact(self.domain[axis])?)?,
            )?;
        let remainder = Interval::exact(1.)?.sub(fraction)?;
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
                        *value = value.mul(remainder)?.add(next.mul(fraction)?)?;
                    }
                }
            }
        }
        let mut left = Self::new(left, work)?;
        let mut right = Self::new(right, work)?;
        left.domain = self.domain;
        right.domain = self.domain;
        left.domain[axis + 1] = coordinate;
        right.domain[axis] = coordinate;
        Ok((left, right))
    }
    fn restrict(&self, domain: [f64; 4], work: &mut usize) -> Result<Self> {
        let mut cell = self.clone();
        for (u, axis) in [(true, 0), (false, 2)] {
            if domain[axis + 1] < cell.domain[axis + 1] {
                cell = cell.split_at(u, domain[axis + 1], work)?.0;
            }
            if domain[axis] > cell.domain[axis] {
                cell = cell.split_at(u, domain[axis], work)?.1;
            }
        }
        Ok(cell)
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
        let width = cell.net[0].len();
        let x = middle(&cell.points[width * last]) - middle(&cell.points[0]);
        let y = middle(&cell.points[width - 1]) - middle(&cell.points[0]);
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

fn homogeneous_hull(net: &[Vec<Homogeneous>]) -> Homogeneous {
    let mut result = net[0][0];
    for h in net.iter().flatten() {
        for (bound, value) in result.iter_mut().zip(h) {
            bound.low = bound.low.min(value.low);
            bound.high = bound.high.max(value.high);
        }
    }
    result
}
fn midpoint_net(net: &[Vec<Homogeneous>], work: &mut usize) -> Result<Homogeneous> {
    fn midpoint(mut row: Vec<Homogeneous>, work: &mut usize) -> Result<Homogeneous> {
        let half = Interval::exact(0.5)?;
        while row.len() > 1 {
            charge(work, row.len() * 8)?;
            for i in 0..row.len() - 1 {
                let next = row[i + 1];
                for (value, next) in row[i].iter_mut().zip(next) {
                    *value = value.mul(half)?.add(next.mul(half)?)?;
                }
            }
            row.pop();
        }
        Ok(row[0])
    }
    let rows = net
        .iter()
        .map(|row| midpoint(row.clone(), work))
        .collect::<Result<Vec<_>>>()?;
    midpoint(rows, work)
}
fn derivative_net(cell: &Cell, u: bool, work: &mut usize) -> Result<Vec<Vec<Homogeneous>>> {
    let rows = cell.net.len() - usize::from(u);
    let columns = cell.net[0].len() - usize::from(!u);
    let degree = Interval::exact(if u { rows } else { columns } as f64)?;
    charge(work, rows * columns * 8)?;
    (0..rows)
        .map(|i| {
            (0..columns)
                .map(|j| {
                    let a = cell.net[i][j];
                    let b = cell.net[i + usize::from(u)][j + usize::from(!u)];
                    Ok([
                        b[0].sub(a[0])?.mul(degree)?,
                        b[1].sub(a[1])?.mul(degree)?,
                        b[2].sub(a[2])?.mul(degree)?,
                        b[3].sub(a[3])?.mul(degree)?,
                    ])
                })
                .collect()
        })
        .collect()
}
fn numerator(position: Homogeneous, derivative: Homogeneous) -> Result<IntervalPoint> {
    Ok([
        derivative[0]
            .mul(position[3])?
            .sub(position[0].mul(derivative[3])?)?,
        derivative[1]
            .mul(position[3])?
            .sub(position[1].mul(derivative[3])?)?,
        derivative[2]
            .mul(position[3])?
            .sub(position[2].mul(derivative[3])?)?,
    ])
}
/// All principal minors of one projected native Jacobian are strictly positive
/// on this rectangle. Candidate axes may round freely: interval evaluation of
/// the complete rational derivative hull establishes the actual certificate.
fn chart_injective(cell: &Cell, work: &mut usize) -> Result<bool> {
    let du = derivative_net(cell, true, work)?;
    let dv = derivative_net(cell, false, work)?;
    let position = homogeneous_hull(&cell.net);
    let nu = numerator(position, homogeneous_hull(&du))?;
    let nv = numerator(position, homogeneous_hull(&dv))?;
    let center = midpoint_net(&cell.net, work)?;
    let a = middle(&numerator(center, midpoint_net(&du, work)?)?);
    let b = middle(&numerator(center, midpoint_net(&dv, work)?)?);
    let la = a.magnitude();
    let lb = b.magnitude();
    if !la.is_finite() || !lb.is_finite() || la <= 0. || lb <= 0. {
        return Ok(false);
    }
    let x = a / la;
    let y = b / lb;
    let cosine = x.dot(y);
    let determinant = 1. - cosine * cosine;
    if !determinant.is_finite() || determinant <= 1e-12 {
        return Ok(false);
    }
    let u = exact((x - y * cosine) / determinant)?;
    let v = exact((y - x * cosine) / determinant)?;
    let aa = dot_intervals(&nu, &u)?;
    let dd = dot_intervals(&nv, &v)?;
    let bb = dot_intervals(&nv, &u)?;
    let cc = dot_intervals(&nu, &v)?;
    Ok(aa.low > 0. && dd.low > 0. && aa.mul(dd)?.sub(bb.mul(cc)?)?.low > 0.)
}
fn union_domain(a: &Cell, b: &Cell) -> [f64; 4] {
    [
        a.domain[0].min(b.domain[0]),
        a.domain[1].max(b.domain[1]),
        a.domain[2].min(b.domain[2]),
        a.domain[3].max(b.domain[3]),
    ]
}
fn atlas_pair(root: &Cell, a: Cell, b: Cell, work: &mut usize) -> Result<()> {
    let mut pending = vec![(a, b, 0usize)];
    let mut visited = 0;
    while let Some((a, b, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_CELLS {
            return Err(unsupported(
                "a native atlas pair exceeds its separation cell budget",
            ));
        }
        if hulls_separate(&a, &b, work)? {
            continue;
        }
        // Adjacent parameter cells may meet at their common parameter edge or
        // vertex. Injectivity of their bounding native rectangle permits exactly
        // that contact; no geometric distance tolerance exempts another contact.
        let joint = root.restrict(union_domain(&a, &b), work)?;
        if chart_injective(&joint, work)? {
            continue;
        }
        if depth >= MAX_DEPTH {
            return Err(unsupported(
                "native chart interiors overlap, or their complete separation cannot be certified within bounded refinement; proof failure does not establish a fold",
            ));
        }
        if a.extent() >= b.extent() {
            let (left, right) = a.split(a.split_direction(), work)?;
            pending.push((right, b.clone(), depth + 1));
            pending.push((left, b, depth + 1));
        } else {
            let (left, right) = b.split(b.split_direction(), work)?;
            pending.push((a.clone(), right, depth + 1));
            pending.push((a, left, depth + 1));
        }
    }
    Ok(())
}
/// Prove a complete positive rational Bernstein carrier with multiple local
/// projection charts, then establish separation between every chart pair.
/// De Casteljau refinement uses outward intervals on the original native net;
/// rounded subpatches or display triangles never become proof authority.
pub(crate) fn prove_carrier_atlas(surface: &NurbsSurface<Vector4>, work: &mut usize) -> Result<()> {
    let root = Cell::from_surface(surface, work)?;
    let mut pending = vec![(root.clone(), 0usize)];
    let mut charts = Vec::new();
    while let Some((cell, depth)) = pending.pop() {
        if chart_injective(&cell, work)? {
            charts.push(cell);
            continue;
        }
        if depth >= 16 || charts.len() + pending.len() + 2 > 1024 {
            return Err(unsupported(
                "local native charts cannot establish regular injective rectangles within their subdivision budget; this does not establish folding",
            ));
        }
        let (left, right) = cell.split(cell.split_direction(), work)?;
        pending.push((right, depth + 1));
        pending.push((left, depth + 1));
    }
    let mut pairs = 0usize;
    for (i, a) in charts.iter().enumerate() {
        for b in charts.iter().skip(i + 1) {
            if boxes_separate(a, b) {
                continue;
            }
            pairs += 1;
            if pairs > MAX_PAIRS {
                return Err(unsupported(
                    "local native atlas exceeds its overlapping-chart pair budget",
                ));
            }
            atlas_pair(&root, a.clone(), b.clone(), work)?;
        }
    }
    Ok(())
}

/// Adjacent faces are exempt only at their shared native connector. A plane
/// through that line must put every other rational control strictly on its
/// own side. The omitted endpoint row is the exact Bernstein factor u or 1-u,
/// so this proves separation up to the common boundary, not an epsilon gap.
fn connector_separates(a: &Cell, b: &Cell, ends: (bool, bool), work: &mut usize) -> Result<bool> {
    if a.net[0].len() != 2 || b.net[0].len() != 2 {
        return Ok(false);
    }
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

#[derive(Clone, Copy)]
enum Contact {
    Edge { u: bool, ends: (bool, bool) },
    Vertex { a: [bool; 2], b: [bool; 2] },
}
fn boundary(index: usize) -> (bool, bool) {
    match index {
        0 => (false, false),
        1 => (true, true),
        2 => (false, true),
        _ => (true, false),
    }
}
fn face_vertices(face: &Face) -> Result<[(VertexId, [bool; 2]); 4]> {
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported("a native chart lost its four-sided boundary"));
    }
    Ok([
        (wires[0][0].front().id(), [false, false]),
        (wires[0][0].back().id(), [true, false]),
        (wires[0][2].front().id(), [true, true]),
        (wires[0][2].back().id(), [false, true]),
    ])
}
fn shared_contact(a: &Face, b: &Face) -> Result<Option<Contact>> {
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
            let (u, end_a) = boundary(i);
            let (v, end_b) = boundary(j);
            if u != v || shared.is_some() {
                return Err(unsupported(
                    "loft charts have unexpected shared boundary topology",
                ));
            }
            shared = Some(Contact::Edge {
                u,
                ends: (end_a, end_b),
            });
        }
    }
    if shared.is_some() {
        return Ok(shared);
    }
    for (a, corner_a) in face_vertices(a)? {
        for (b, corner_b) in face_vertices(b)? {
            if a != b {
                continue;
            }
            if shared.is_some() {
                return Err(unsupported(
                    "charts meet at multiple isolated native vertices without shared boundary authority",
                ));
            }
            shared = Some(Contact::Vertex {
                a: corner_a,
                b: corner_b,
            });
        }
    }
    Ok(shared)
}

fn transpose(cell: &Cell, work: &mut usize) -> Result<Cell> {
    Cell::new(
        (0..cell.net[0].len())
            .map(|j| cell.net.iter().map(|row| row[j]).collect())
            .collect(),
        work,
    )
}
fn vertex_separates(
    a: &Cell,
    b: &Cell,
    corners: ([bool; 2], [bool; 2]),
    work: &mut usize,
) -> Result<bool> {
    let corner = |cell: &Cell, ends: [bool; 2]| {
        [
            if ends[0] { cell.net.len() - 1 } else { 0 },
            if ends[1] { cell.net[0].len() - 1 } else { 0 },
        ]
    };
    let ia = corner(a, corners.0);
    let ib = corner(b, corners.1);
    if !same_projective_point(&a.net[ia[0]][ia[1]], &b.net[ib[0]][ib[1]]) {
        return Ok(false);
    }
    let anchor = physical(&a.net[ia[0]][ia[1]])?;
    let da = middle(&a.bounds) - middle(&anchor);
    let db = middle(&b.bounds) - middle(&anchor);
    for candidate in [db - da, db, -da] {
        let length = candidate.magnitude();
        if !length.is_finite() || length <= 1e-12 {
            continue;
        }
        let axis = exact(candidate / length)?;
        let mut separated = true;
        for (cell, corner, negative) in [(a, ia, true), (b, ib, false)] {
            for (i, row) in cell.net.iter().enumerate() {
                for (j, h) in row.iter().enumerate() {
                    if [i, j] == corner {
                        continue;
                    }
                    charge(work, 36)?;
                    let value = dot_intervals(&difference(&physical(h)?, &anchor)?, &axis)?;
                    separated &= if negative {
                        value.high < 0.
                    } else {
                        value.low > 0.
                    };
                }
            }
        }
        // The omitted coefficient is the one exact common Bernstein corner.
        // Every other basis coefficient is strictly on its own side, proving
        // contact only at this vertex throughout both complete rectangles.
        if separated {
            return Ok(true);
        }
    }
    Ok(false)
}
fn joint_chart(a: &Face, b: &Face, contact: Contact, work: &mut usize) -> Result<bool> {
    let Contact::Edge { u, ends } = contact else {
        return Ok(false);
    };
    let (Surface::NurbsSurface(a), Surface::NurbsSurface(b)) = (a.surface(), b.surface()) else {
        return Ok(false);
    };
    let mut aa = a.control_points().clone();
    let mut bb = b.control_points().clone();
    if !u {
        let transpose = |net: Vec<Vec<Vector4>>| {
            (0..net[0].len())
                .map(|j| net.iter().map(|row| row[j]).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        aa = transpose(aa);
        bb = transpose(bb);
    }
    let (p, q) = (aa.len() - 1, aa[0].len() - 1);
    if bb.len() != p + 1 || bb.iter().any(|row| row.len() != q + 1) {
        return Ok(false);
    }
    if !ends.0 {
        aa.reverse();
    }
    if ends.1 {
        bb.reverse();
    }
    if aa[p] != bb[0] {
        let mut reverse = bb[0].clone();
        reverse.reverse();
        if aa[p] != reverse {
            return Ok(false);
        }
        for row in &mut bb {
            row.reverse();
        }
    }
    // An exact common native control row makes this a continuous piecewise
    // rational rectangle. C0 joins use the full projected monotonicity proof,
    // not merely independent local Jacobian signs.
    aa.pop();
    aa.extend(bb);
    let knots = std::iter::repeat_n(0., p + 1)
        .chain(std::iter::repeat_n(1., p))
        .chain(std::iter::repeat_n(2., p + 1))
        .collect::<Vec<_>>();
    let carrier = NurbsSurface::new(BsplineSurface::new(
        (KnotVector::from(knots), KnotVector::bezier_knot(q)),
        aa,
    ));
    match crate::surface_construct::prove_regular(&carrier, work) {
        Ok(()) => Ok(true),
        Err(CadError::Unsupported(_)) => Ok(false),
        Err(error) => Err(error),
    }
}
fn contact_separates(a: &Cell, b: &Cell, contact: Contact, work: &mut usize) -> Result<bool> {
    match contact {
        Contact::Edge { u: true, ends } => connector_separates(a, b, ends, work),
        Contact::Edge { u: false, ends } => {
            connector_separates(&transpose(a, work)?, &transpose(b, work)?, ends, work)
        }
        Contact::Vertex { a: ca, b: cb } => vertex_separates(a, b, (ca, cb), work),
    }
}
fn retained_contact(
    contact: Option<Contact>,
    first: bool,
    u: bool,
    right: bool,
) -> Option<Contact> {
    contact.filter(|contact| match *contact {
        Contact::Edge { u: axis, ends } => {
            axis != u
                || if first {
                    ends.0 == right
                } else {
                    ends.1 == right
                }
        }
        Contact::Vertex { a, b } => {
            if first {
                a[usize::from(!u)] == right
            } else {
                b[usize::from(!u)] == right
            }
        }
    })
}
fn pair(a: Cell, b: Cell, shared: Option<Contact>, work: &mut usize) -> Result<()> {
    let mut pending = vec![(a, b, shared, 0usize)];
    let mut visited = 0;
    while let Some((a, b, shared, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_CELLS {
            return Err(unsupported(
                "a native face pair exceeded the separation cell budget",
            ));
        }
        if let Some(contact) = shared {
            if contact_separates(&a, &b, contact, work)? {
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
        let u = match shared {
            Some(Contact::Edge { u, .. }) => u,
            _ => {
                if split_a {
                    a.split_direction()
                } else {
                    b.split_direction()
                }
            }
        };
        if split_a {
            let (left, right) = a.split(u, work)?;
            let left_shared = retained_contact(shared, true, u, false);
            let right_shared = retained_contact(shared, true, u, true);
            pending.push((right, b.clone(), right_shared, depth + 1));
            pending.push((left, b, left_shared, depth + 1));
        } else {
            let (left, right) = b.split(u, work)?;
            let left_shared = retained_contact(shared, false, u, false);
            let right_shared = retained_contact(shared, false, u, true);
            pending.push((a.clone(), right, right_shared, depth + 1));
            pending.push((a, left, left_shared, depth + 1));
        }
    }
    Ok(())
}

/// Certify mutual separation of every native face, including different section
/// pairs. Each face separately needs its complete native injectivity proof.
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
            let shared = shared_contact(&skin[a], &skin[b])?;
            if let Some(contact) = shared
                && joint_chart(&skin[a], &skin[b], contact, work)?
            {
                continue;
            }
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

fn grow_exact(expansion: &mut Vec<f64>, mut value: f64) -> Result<()> {
    let mut output = Vec::with_capacity(expansion.len() + 1);
    for component in expansion.iter().copied() {
        let sum = value + component;
        if !sum.is_finite() {
            return Err(unsupported("exact plane-functional arithmetic overflowed"));
        }
        let virtual_component = sum - value;
        let error = (value - (sum - virtual_component)) + (component - virtual_component);
        if error != 0. {
            output.push(error);
        }
        value = sum;
    }
    if value != 0. {
        output.push(value);
    }
    *expansion = output;
    Ok(())
}
fn scale_exact(expansion: &[f64], scale: f64) -> Result<Vec<f64>> {
    let mut result = Vec::new();
    for value in expansion {
        let product = value * scale;
        // Error-free FMA products require representable tails. Refuse extreme
        // scales rather than treating an underflowed determinant as exact zero.
        if !product.is_finite() || (*value != 0. && scale != 0. && product.abs() < 1e-250) {
            return Err(unsupported(
                "exact cap-plane arithmetic exceeds its safe exponent range",
            ));
        }
        grow_exact(&mut result, value.mul_add(scale, -product))?;
        grow_exact(&mut result, product)?;
    }
    Ok(result)
}
struct CapPlane {
    origin: Vector3,
    u: Vector3,
    v: Vector3,
    normal: IntervalPoint,
}
impl CapPlane {
    fn new(face: &Face) -> Result<Self> {
        let Surface::Plane(plane) = face.oriented_surface() else {
            return Err(unsupported(
                "solid loft end caps need native planar carriers",
            ));
        };
        let u = plane.axis_u();
        let v = plane.axis_v();
        Ok(Self {
            origin: plane.origin().to_vec(),
            u,
            v,
            normal: cross(&exact(u)?, &exact(v)?)?,
        })
    }
    fn zero(&self, h: Vector4, work: &mut usize) -> Result<bool> {
        charge(work, 256)?;
        let mut delta = Vec::new();
        for i in 0..3 {
            let mut value = scale_exact(&[self.origin[i]], -h.w)?;
            grow_exact(&mut value, h[i])?;
            delta.push(value);
        }
        let mut result = Vec::new();
        for (i, j, k, sign) in [
            (0, 1, 2, 1.),
            (0, 2, 1, -1.),
            (1, 2, 0, 1.),
            (1, 0, 2, -1.),
            (2, 0, 1, 1.),
            (2, 1, 0, -1.),
        ] {
            for value in scale_exact(&scale_exact(&delta[k], self.v[j])?, self.u[i] * sign)? {
                grow_exact(&mut result, value)?;
            }
        }
        Ok(result.is_empty())
    }
    fn value(&self, h: Vector4) -> Result<Interval> {
        let h = [
            Interval::exact(h.x)?,
            Interval::exact(h.y)?,
            Interval::exact(h.z)?,
            Interval::exact(h.w)?,
        ];
        let origin = exact(self.origin)?;
        let relative = [
            h[0].sub(origin[0].mul(h[3])?)?,
            h[1].sub(origin[1].mul(h[3])?)?,
            h[2].sub(origin[2].mul(h[3])?)?,
        ];
        dot_intervals(&relative, &self.normal)
    }
}
/// A nonparallel solid loft needs separation from its end caps as well as
/// between skins. The actual plane functional vanishes on every shared native
/// boundary coefficient; all other coefficients have a strict interior sign.
/// This factors the common boundary without a tolerance band or omitted collar.
pub(crate) fn prove_end_caps(
    skin: &Shell,
    start: &Face,
    end: &Face,
    work: &mut usize,
) -> Result<()> {
    use monstertruck::geometry::prelude::TryIntoHomogeneousBsplineCurve;
    if skin.is_empty() {
        return Err(unsupported(
            "solid loft cap separation requires a complete nonempty native skin",
        ));
    }
    for (cap, positive) in [(start, true), (end, false)] {
        let plane = CapPlane::new(cap)?;
        let edges = cap
            .edge_iter()
            .map(|e| e.id())
            .collect::<std::collections::HashSet<_>>();
        for edge in cap.edge_iter() {
            let curve = edge
                .curve()
                .try_into_homogeneous_bspline_curve()
                .ok_or_else(|| {
                    unsupported("loft cap boundaries need explicit native rational carriers")
                })?;
            for h in curve.control_points() {
                if h.w <= 0. || !plane.zero(*h, work)? {
                    return Err(unsupported(
                        "cap boundary coefficients are not exactly on their represented native plane; a general trim collar certificate is required",
                    ));
                }
            }
        }
        for face in skin {
            let Surface::NurbsSurface(carrier) = face.surface() else {
                return Err(unsupported("loft skin lost its native rational carrier"));
            };
            let wires = face.boundaries();
            if wires.len() != 1 || wires[0].len() != 4 {
                return Err(unsupported("loft skin lost its native four-sided trim"));
            }
            let shared = wires[0]
                .iter()
                .enumerate()
                .filter(|(_, e)| edges.contains(&e.id()))
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if shared.len() > 1 || shared.first().is_some_and(|i| ![0, 2].contains(i)) {
                return Err(unsupported(
                    "end-cap contact has unsupported native boundary topology",
                ));
            }
            let column = shared.first().map(|i| {
                if *i == 0 {
                    0
                } else {
                    carrier.control_points()[0].len() - 1
                }
            });
            for row in carrier.control_points() {
                for (j, h) in row.iter().enumerate() {
                    charge(work, 48)?;
                    if column == Some(j) {
                        if !plane.zero(*h, work)? {
                            return Err(unsupported(
                                "skin and cap do not share an exact plane boundary; complete native collar separation is unresolved",
                            ));
                        }
                        continue;
                    }
                    let value = plane.value(*h)?;
                    if !(if positive {
                        value.low > 0.
                    } else {
                        value.high < 0.
                    }) {
                        return Err(unsupported(
                            "the skin approaches an end cap away from its shared boundary, or a strict native cap halfspace certificate is unavailable",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
