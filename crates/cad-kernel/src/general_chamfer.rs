//! Exact conical bevels and toroidal rounds on turned rims.
//! Circular controls remain rational; shared rails are built once and reused.
//! No relay points, sampled strips, or display meshes are construction inputs.
use crate::{planar::ChamferSpec, shell_history::ShellHistory};
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::{HashMap, HashSet};
#[path = "circular_caps.rs"]
mod circular_caps;
#[path = "circular_terminals.rs"]
mod circular_terminals;
#[path = "concave_treatment.rs"]
mod concave_treatment;
#[path = "local_blend.rs"]
mod local_blend;
#[path = "straight_chains.rs"]
mod straight_chains;

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_CONTROLS: usize = 131_072;
const MAX_PROOF_WORK: usize = 4_000_000;

#[derive(Clone, Copy)]
pub(crate) enum Treatment {
    Chamfer,
    Fillet,
}
impl Treatment {
    fn name(self) -> &'static str {
        match self {
            Self::Chamfer => "chamfer",
            Self::Fillet => "fillet",
        }
    }
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Circular edge treatment: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Circular edge treatment: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "A carrier requires finite positive rational weights",
        ));
    }
    let p = Point3::from_vec(h.truncate() / h.w);
    if ![p.x, p.y, p.z].iter().all(|v| v.is_finite()) {
        return Err(invalid("A carrier exceeds finite coordinates"));
    }
    Ok(p)
}

struct Wall {
    face: Face,
    net: BsplineSurface<Vector4>,
    ends: [Point3; 2],
    radii: [f64; 2],
    cuts: [f64; 2],
    rims: [Edge; 2],
    offsets: [Vector3; 2],
}
struct Rim {
    cap: Face,
    edge: Edge,
    center: Point3,
    axis: Vector3,
    radius: f64,
    wall_radius: f64,
    profile_weight: f64,
    radial: f64,
    depth: f64,
    side: f64,
    axial_side: f64,
    wall: FaceId,
    column: usize,
    rails: Option<Rails>,
    bounds: Option<BandBounds>,
}
impl Rim {
    fn rails(&self) -> Result<&Rails> {
        self.rails
            .as_ref()
            .ok_or_else(|| invalid("Circular rails were not prepared"))
    }
    fn bounds(&self) -> Result<&BandBounds> {
        self.bounds
            .as_ref()
            .ok_or_else(|| invalid("Circular band bounds were not prepared"))
    }
}

struct Rails {
    original: BsplineCurve<Vector4>,
    planar: BsplineCurve<Vector4>,
    turned: BsplineCurve<Vector4>,
    middle: Option<BsplineCurve<Vector4>>,
    source_edge: Curve,
    exact_cut: crate::native_bounds::CurveBounds,
    source_strip: Vec<CurveGroup>,
    forward: bool,
    connectors: Vec<Curve>,
}
struct BandBounds {
    axis: crate::native_bounds::IntervalPoint,
    axial: crate::native_bounds::Interval,
    inner: f64,
    outer: f64,
}
fn rail_curve(curve: &BsplineCurve<Vector4>) -> Curve {
    Curve::NurbsCurve(NurbsCurve::new(curve.clone()))
}
fn certify_profile(rim: &Rim, rails: &Rails) -> Result<()> {
    use crate::native_bounds::{CurveBounds, Interval, exact_point};
    let center = exact_point(rim.center)?;
    let axis = exact_point(Point3::from_vec(rim.axis))?;
    let scale = Interval::exact(rim.wall_radius)?.div(Interval::exact(rim.radius)?)?;
    let depth = Interval::exact(rim.axial_side)?.mul(Interval::exact(rim.depth)?)?;
    let mut reference = Vec::new();
    let mut actual = Vec::new();
    for (index, source) in rails.original.control_points().iter().enumerate() {
        let mut intended = [
            Interval::exact(source.x)?,
            Interval::exact(source.y)?,
            Interval::exact(source.z)?,
            Interval::exact(source.w)?,
        ];
        for coordinate in 0..3 {
            let origin = center[coordinate].mul(intended[3])?;
            intended[coordinate] = origin
                .add(intended[coordinate].sub(origin)?.mul(scale)?)?
                .add(axis[coordinate].mul(depth)?.mul(intended[3])?)?;
        }
        reference.push(intended);
        actual.push(rails.turned.control_points()[index]);
        for control in std::iter::once(&rails.planar.control_points()[index]).chain(
            rails
                .middle
                .as_ref()
                .map(|middle| &middle.control_points()[index]),
        ) {
            reference.push([
                Interval::exact(control.x)?,
                Interval::exact(control.y)?,
                Interval::exact(control.z)?,
                Interval::exact(control.w)?,
            ]);
            actual.push(*control);
        }
    }
    if CurveBounds::control_net_distance(&reference, &actual, rim.center)? > EPS {
        return Err(unsupported(
            "The native profile cannot certify the requested circular setback within modeling tolerance",
        ));
    }
    Ok(())
}
impl Rails {
    fn build(rim: &Rim, wall: &Wall, treatment: Treatment, connectors: Vec<Curve>) -> Result<Self> {
        let source = rim.edge.curve();
        if !matches!(
            source,
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        ) {
            return Err(unsupported(
                "Circular construction requires a stored native rational rim",
            ));
        }
        let source_count = match &source {
            Curve::Line(_) => 2,
            Curve::BsplineCurve(curve) => curve.control_points().len(),
            Curve::NurbsCurve(curve) => curve.control_points().len(),
            _ => 0,
        };
        if source_count.saturating_mul(4) > MAX_CONTROLS {
            return Err(unsupported("Circular rails exceed their control budget"));
        }
        // All profile rails share the wall's angular frame. A selected edge
        // may trace the same bounded circle with entirely different knots.
        let columns = [wall.net.curve_u(0), wall.net.curve_u(1)];
        let original = columns[rim.column].clone();
        let (start, end) = original.range_tuple();
        let start = point(original.subs(start))?;
        let end = point(original.subs(end))?;
        let front = rim.edge.absolute_front().point();
        let back = rim.edge.absolute_back().point();
        let forward = start.distance(front) <= EPS && end.distance(back) <= EPS;
        let reverse = start.distance(back) <= EPS && end.distance(front) <= EPS;
        if forward == reverse {
            return Err(unsupported(
                "The native wall frame has ambiguous rim endpoints",
            ));
        }
        if original.control_points().len().saturating_mul(4) > MAX_CONTROLS {
            return Err(unsupported("Circular rails exceed their control budget"));
        }
        let mut planar = original.clone();
        let scale = 1. - rim.side * rim.radial / rim.radius;
        planar.transform_control_points(|h| {
            let center = rim.center.to_vec() * h.w;
            *h = (center + (h.truncate() - center) * scale).extend(h.w);
        });
        let cut = wall.cuts[rim.column];
        let mut turned = BsplineCurve::new(
            original.knot_vector().clone(),
            columns[0]
                .control_points()
                .iter()
                .zip(columns[1].control_points())
                .map(|(a, b)| *a * (1. - cut) + *b * cut)
                .collect(),
        );
        // An axial NURBS section can carry a global weight scale. Restore the
        // source angular denominator before using it as a conic profile end.
        let weight_scale = original.control_points()[0].w / turned.control_points()[0].w;
        turned.transform_control_points(|h| *h *= weight_scale);
        let exact_cut =
            crate::native_bounds::CurveBounds::linear_section(&columns[0], &columns[1], cut)?
                .scale_homogeneous(weight_scale)?;
        if exact_cut.distance_from(&turned, rim.center)? > EPS {
            return Err(unsupported(
                "A native wall cut cannot be represented within modeling tolerance",
            ));
        }
        let middle = matches!(treatment, Treatment::Fillet).then(|| {
            let mut curve = original.clone();
            curve.transform_control_points(|h| *h *= rim.profile_weight);
            curve
        });
        let source_strip = if matches!(wall.face.surface(), Surface::RevolutionSurface(_)) {
            // Validation uses Monstertruck's declared coordinate tolerance;
            // account for that complete residual when locating source trims.
            crate::native_bounds::source_strip(
                &wall.face.surface(),
                &source,
                &rail_curve(&turned),
                monstertruck::core::tolerance::TOLERANCE,
            )?
        } else {
            Vec::new()
        };
        let rails = Self {
            original,
            planar,
            turned,
            middle,
            source_edge: source,
            exact_cut,
            source_strip,
            forward,
            connectors,
        };
        certify_profile(rim, &rails)?;
        // Certify positivity after the actual native homogeneous arithmetic,
        // including weighted middle controls which can otherwise underflow.
        for curve in rails.groups()?.into_iter().flatten() {
            crate::native_bounds::subdivision_curve(&curve)?
                .ok_or_else(|| unsupported("A generated rail has no native rational enclosure"))?;
        }
        Ok(rails)
    }
    fn groups(&self) -> Result<Vec<Vec<Curve>>> {
        let mut profile = vec![
            rail_curve(&self.original),
            rail_curve(&self.planar),
            rail_curve(&self.turned),
        ];
        if let Some(middle) = &self.middle {
            profile.push(rail_curve(middle));
        }
        let mut groups = vec![profile, vec![self.source_edge.clone()]];
        // Enclose each entire seam wedge, not just its three boundary curves.
        // Constant curves retain all native homogeneous connector controls in
        // one convex group without inventing a shared parameterization.
        for seam in self.connectors.chunks(3) {
            let mut controls = Vec::new();
            for curve in seam {
                let curve = curve
                    .clone()
                    .try_into_homogeneous_bspline_curve()
                    .ok_or_else(|| unsupported("A seam has no native rational controls"))?;
                controls.extend(
                    curve.control_points().iter().map(|h| {
                        rail_curve(&BsplineCurve::new(KnotVector::bezier_knot(0), vec![*h]))
                    }),
                );
            }
            groups.push(controls);
        }
        Ok(groups)
    }
    fn count(&self) -> usize {
        self.original
            .control_points()
            .len()
            .saturating_mul(if self.middle.is_some() { 4 } else { 3 })
            .saturating_add(self.exact_cut.count())
            .saturating_add(
                self.source_strip
                    .iter()
                    .flatten()
                    .map(|curve| curve.count())
                    .sum::<usize>(),
            )
            .saturating_add(match &self.source_edge {
                Curve::Line(_) => 2,
                Curve::BsplineCurve(c) => c.control_points().len(),
                Curve::NurbsCurve(c) => c.control_points().len(),
                _ => MAX_CONTROLS,
            })
            .saturating_add(self.connectors.len().saturating_mul(3))
    }
}

type CurveGroup = Vec<crate::native_bounds::CurveBounds>;
fn interval_groups(rim: &Rim) -> Result<Vec<CurveGroup>> {
    let rails = rim.rails()?;
    let mut groups: Vec<CurveGroup> = rails
        .groups()?
        .into_iter()
        .map(|group| {
            group
                .iter()
                .map(|curve| {
                    crate::native_bounds::subdivision_curve(curve)?.ok_or_else(|| {
                        unsupported("A treatment rail has no native subdivision enclosure")
                    })
                })
                .collect()
        })
        .collect::<Result<_>>()?;
    groups[0].push(rails.exact_cut.clone());
    groups.extend(rails.source_strip.clone());
    Ok(groups)
}
fn group_hull(group: &CurveGroup, work: &mut usize) -> Result<Vec<Point3>> {
    let count = group
        .iter()
        .map(|curve| curve.count().saturating_mul(8))
        .sum::<usize>();
    *work = work.saturating_add(count);
    if count > MAX_CONTROLS || *work > MAX_PROOF_WORK {
        return Err(unsupported(
            "Circular control-hull proof exceeds its work budget",
        ));
    }
    let mut hull = Vec::with_capacity(count);
    for curve in group {
        hull.extend_from_slice(curve.enclosure()?.points());
    }
    Ok(hull)
}
fn bisect_group(group: CurveGroup, work: &mut usize) -> Result<Option<[CurveGroup; 2]>> {
    let mut halves = [Vec::new(), Vec::new()];
    for curve in group {
        let Some([a, b]) = curve.bisect(work, MAX_PROOF_WORK)? else {
            return Ok(None);
        };
        halves[0].push(a);
        halves[1].push(b);
    }
    Ok(Some(halves))
}

fn band_bounds(rim: &Rim, work: &mut usize) -> Result<BandBounds> {
    use crate::native_bounds::{Interval, dot_intervals, exact_point, scaled_norm};
    let raw = exact_point(Point3::from_vec(rim.axis))?;
    let length = dot_intervals(&raw, &raw)?.sqrt()?;
    let axis = [
        raw[0].div(length)?,
        raw[1].div(length)?,
        raw[2].div(length)?,
    ];
    let center = exact_point(rim.center)?;
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    let mut inner = f64::INFINITY;
    let mut outer = 0.0f64;
    let mut stack = interval_groups(rim)?
        .into_iter()
        .map(|group| (group, 0usize))
        .collect::<Vec<_>>();
    while let Some((group, depth)) = stack.pop() {
        if depth < 6 {
            let Some([a, b]) = bisect_group(group, work)? else {
                return Err(unsupported(
                    "The circular band cannot resolve a bounded subdivision",
                ));
            };
            stack.push((a, depth + 1));
            stack.push((b, depth + 1));
            continue;
        }
        let hull = group_hull(&group, work)?;
        let mut radial = Vec::with_capacity(hull.len());
        let mut guide = Vector3::zero();
        for point in &hull {
            let p = exact_point(*point)?;
            let offset = [
                p[0].sub(center[0])?,
                p[1].sub(center[1])?,
                p[2].sub(center[2])?,
            ];
            let along = dot_intervals(&offset, &axis)?;
            low = low.min(along.low);
            high = high.max(along.high);
            let perpendicular = [
                offset[0].sub(axis[0].mul(along)?)?,
                offset[1].sub(axis[1].mul(along)?)?,
                offset[2].sub(axis[2].mul(along)?)?,
            ];
            outer = outer.max(scaled_norm(1., &perpendicular)?);
            radial.push(perpendicular);
            let delta = *point - rim.center;
            guide += delta - rim.axis * delta.dot(rim.axis);
        }
        guide /= hull.len() as f64;
        let lower = if [guide.x, guide.y, guide.z]
            .iter()
            .all(|value| value.is_finite())
        {
            let direction = exact_point(Point3::from_vec(guide))?;
            let norm = scaled_norm(1., &direction)?;
            if norm > 0. {
                let mut support = f64::INFINITY;
                for point in &radial {
                    support = support.min(dot_intervals(point, &direction)?.low);
                }
                if support > 0. {
                    Interval::exact(support)?
                        .div(Interval::exact(norm)?)?
                        .low
                        .max(0.)
                } else {
                    0.
                }
            } else {
                0.
            }
        } else {
            0.
        };
        inner = inner.min(lower);
    }
    Ok(BandBounds {
        axis,
        axial: Interval::new(low, high)?,
        inner,
        outer,
    })
}

fn carrier(face: &Face) -> Result<Wall> {
    let mut net = match face.surface() {
        Surface::BsplineSurface(_) | Surface::NurbsSurface(_) => face
            .oriented_surface()
            .try_into_homogeneous_bspline_surface()
            .ok_or_else(|| unsupported("A rim wall needs a native rational turned carrier"))?,
        Surface::RevolutionSurface(processor)
            if matches!(processor.entity().entity_curve(), Curve::Line(_)) =>
        {
            revolution_frame(face)?
        }
        _ => {
            return Err(unsupported(
                "Circular trimming requires a native rational or line-revolution wall",
            ));
        }
    };
    if net
        .control_points()
        .len()
        .saturating_mul(net.control_points().first().map_or(0, Vec::len))
        > MAX_CONTROLS
    {
        return Err(unsupported(
            "The turned carrier exceeds the rational control budget",
        ));
    }
    if net.control_points().is_empty() || net.control_points()[0].is_empty() {
        return Err(invalid("The turned carrier has no controls"));
    }
    let transposed = net.vdegree() != 1 || net.control_points()[0].len() != 2;
    if transposed {
        net.invert();
    }
    if net.vdegree() != 1 || net.control_points()[0].len() != 2 || !net.is_clamped() {
        return Err(unsupported(
            "A rim wall needs a clamped linear axial parameter",
        ));
    }
    let curves: Vec<_> = (0..2)
        .map(|column| {
            BsplineCurve::try_new(
                net.knot_vector_u().clone(),
                net.control_points().iter().map(|row| row[column]).collect(),
            )
            .map(|curve| Curve::NurbsCurve(NurbsCurve::new(curve)))
            .map_err(|_| unsupported("A rim wall has invalid circular column knots"))
        })
        .collect::<Result<_>>()?;
    let circles = [
        crate::bounded_carriers::circle(&curves[0])?.0,
        crate::bounded_carriers::circle(&curves[1])?.0,
    ];
    let delta = circles[1].0 - circles[0].0;
    let length = delta.magnitude();
    if !length.is_finite()
        || length <= EPS
        || circles
            .iter()
            .any(|(_, normal, _)| normal.dot(delta / length).abs() < 1. - 1e-10)
    {
        return Err(unsupported(
            "The adjacent carrier is not an exact cylinder or cone",
        ));
    }
    let weight_ratio = net.control_points()[0][1].w / net.control_points()[0][0].w;
    let radial_ratio = circles[1].2 / circles[0].2;
    if !radial_ratio.is_finite()
        || radial_ratio <= 0.
        || !weight_ratio.is_finite()
        || weight_ratio <= 0.
    {
        return Err(unsupported(
            "A turned wall reaches or crosses its cone apex",
        ));
    }
    for row in net.control_points() {
        if (point(row[1])? - circles[1].0 - (point(row[0])? - circles[0].0) * radial_ratio)
            .magnitude()
            > EPS
            || (row[1].w / row[0].w - weight_ratio).abs() > 1e-12 * weight_ratio.abs()
        {
            return Err(unsupported(
                "Circular wall columns do not prove an exact coaxial homothety",
            ));
        }
    }
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "A turned patch needs two bounded circular rims and two axial seams",
        ));
    }
    let mut rims = Vec::new();
    for (column, circle) in circles.iter().enumerate() {
        let matches: Vec<_> = wires[0]
            .iter()
            .filter(|edge| {
                crate::bounded_carriers::circle(&edge.curve()).is_ok_and(|(c, _)| {
                    c.0.distance(circle.0) <= EPS
                        && (c.2 - circle.2).abs() <= EPS
                        && c.1.dot(circle.1).abs() > 1. - 1e-10
                }) && corresponding_arc(edge, &curves[column])
            })
            .cloned()
            .collect();
        if matches.len() != 1 {
            return Err(unsupported(
                "The turned wall trim does not follow its exact end circles",
            ));
        }
        rims.push(matches[0].clone());
    }
    let source_rims: HashSet<_> = rims.iter().map(Edge::id).collect();
    for edge in &wires[0] {
        if source_rims.contains(&edge.id()) {
            continue;
        }
        crate::bounded_carriers::line(&edge.curve())?;
        let natural_seam = [false, true].iter().any(|at_end| {
            let endpoints = curves
                .iter()
                .map(|curve| {
                    let (a, b) = curve.range_tuple();
                    curve.subs(if *at_end { b } else { a })
                })
                .collect::<Vec<_>>();
            (edge.front().point().distance(endpoints[0]) <= EPS
                && edge.back().point().distance(endpoints[1]) <= EPS)
                || (edge.back().point().distance(endpoints[0]) <= EPS
                    && edge.front().point().distance(endpoints[1]) <= EPS)
        });
        if !natural_seam {
            return Err(unsupported(
                "A turned wall trim does not follow its complete natural generatrix",
            ));
        }
    }
    Ok(Wall {
        face: face.clone(),
        net,
        ends: [circles[0].0, circles[1].0],
        radii: [circles[0].2, circles[1].2],
        cuts: [0., 1.],
        rims: [rims[0].clone(), rims[1].clone()],
        offsets: [Vector3::zero(); 2],
    })
}

/// Construction frame only: a line revolution's native analytic carrier is
/// retained and enclosed independently. Its full-circle rational conversion
/// cannot be used as the bounded angular frame of a trimmed native face.
fn revolution_frame(face: &Face) -> Result<BsplineSurface<Vector4>> {
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "A line-revolution rim requires two circular rails and two generatrices",
        ));
    }
    let circles = wires[0]
        .iter()
        .filter_map(|edge| {
            crate::bounded_carriers::circle(&edge.curve())
                .ok()
                .map(|circle| (edge, circle.0))
        })
        .collect::<Vec<_>>();
    if circles.len() != 2 {
        return Err(unsupported(
            "The line revolution has no unique pair of bounded circular ends",
        ));
    }
    let first = circles[0].0.curve();
    if !matches!(
        first,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return Err(unsupported(
            "The revolution rim needs stored native rational angular controls",
        ));
    }
    let source_count = match &first {
        Curve::Line(_) => 2,
        Curve::BsplineCurve(curve) => curve.control_points().len(),
        Curve::NurbsCurve(curve) => curve.control_points().len(),
        _ => MAX_CONTROLS,
    };
    if source_count.saturating_mul(2) > MAX_CONTROLS {
        return Err(unsupported(
            "The revolution frame exceeds its control budget",
        ));
    }
    let curve = first
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("The revolution rim has no native angular frame"))?;
    if curve.control_points().len().saturating_mul(2) > MAX_CONTROLS {
        return Err(unsupported(
            "The revolution frame exceeds its control budget",
        ));
    }
    let (a, _, ra) = circles[0].1;
    let (b, _, rb) = circles[1].1;
    let scale = rb / ra;
    let controls = curve
        .control_points()
        .iter()
        .map(|h| {
            let other = (b.to_vec() * h.w + (h.truncate() - a.to_vec() * h.w) * scale).extend(h.w);
            vec![*h, other]
        })
        .collect();
    Ok(BsplineSurface::new(
        (curve.knot_vector().clone(), KnotVector::bezier_knot(1)),
        controls,
    ))
}

/// A positive rational control hull lies outside the complete annular bevel
/// slab when one of these separating linear/convex bounds succeeds.
fn separated(hull: &[Point3], rim: &Rim, work: &mut usize) -> Result<bool> {
    use crate::native_bounds::{Interval, dot_intervals, exact_point, scaled_norm};
    if hull.is_empty() {
        return Ok(false);
    }
    *work = work.saturating_add(hull.len());
    if hull.len() > MAX_CONTROLS || *work > MAX_PROOF_WORK {
        return Err(unsupported(
            "Collision separation exceeds its control budget",
        ));
    }
    let bounds = rim.bounds()?;
    let center = exact_point(rim.center)?;
    let axis = bounds.axis;
    let slab = bounds.axial.widen(EPS)?;
    let (low, high) = (slab.low, slab.high);
    let inner = Interval::exact(bounds.inner)?
        .sub(Interval::exact(EPS)?)?
        .low;
    let outer = Interval::exact(bounds.outer)?
        .add(Interval::exact(EPS)?)?
        .high;
    let mut above = true;
    let mut below = true;
    let mut within = true;
    let mut radial = Vec::with_capacity(hull.len());
    for point in hull {
        let point = exact_point(*point)?;
        let offset = [
            point[0].sub(center[0])?,
            point[1].sub(center[1])?,
            point[2].sub(center[2])?,
        ];
        let along = dot_intervals(&offset, &axis)?;
        above &= along.low > high;
        below &= along.high < low;
        let perpendicular = [
            offset[0].sub(axis[0].mul(along)?)?,
            offset[1].sub(axis[1].mul(along)?)?,
            offset[2].sub(axis[2].mul(along)?)?,
        ];
        within &= scaled_norm(1., &perpendicular)? < inner;
        radial.push(perpendicular);
    }
    if above || below || within {
        return Ok(true);
    }
    // Candidate directions only guide the search. Every accepted separating
    // plane is checked against the full interval hull, with its norm enclosed.
    let candidates = hull
        .iter()
        .map(|point| {
            let offset = *point - rim.center;
            offset - rim.axis * offset.dot(rim.axis)
        })
        .collect::<Vec<_>>();
    let mean = candidates.iter().copied().sum::<Vector3>() / candidates.len() as f64;
    let stride = candidates.len().div_ceil(16).max(1);
    for direction in std::iter::once(mean).chain(candidates.into_iter().step_by(stride)) {
        if ![direction.x, direction.y, direction.z]
            .iter()
            .all(|x| x.is_finite())
        {
            continue;
        }
        let direction = exact_point(Point3::from_vec(direction))?;
        let length = scaled_norm(1., &direction)?;
        if length <= EPS {
            continue;
        }
        let threshold = Interval::exact(outer)?.mul(Interval::exact(length)?)?.high;
        let mut beyond = true;
        for point in &radial {
            *work = work.saturating_add(1);
            if *work > MAX_PROOF_WORK {
                return Err(unsupported(
                    "Collision separation exceeds its predicate budget",
                ));
            }
            if dot_intervals(point, &direction)?.low <= threshold {
                beyond = false;
                break;
            }
        }
        if beyond {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Equality of circle, endpoints and non-retracing angular extent establishes
/// the complete bounded arc, independently of its rational parameterization.
fn corresponding_arc(edge: &Edge, carrier: &Curve) -> bool {
    let Ok(((_, edge_normal, _), edge_angle)) =
        crate::bounded_carriers::circle(&edge.oriented_curve())
    else {
        return false;
    };
    let Ok(((_, carrier_normal, _), carrier_angle)) = crate::bounded_carriers::circle(carrier)
    else {
        return false;
    };
    let (a, b) = carrier.range_tuple();
    let start = carrier.subs(a);
    let end = carrier.subs(b);
    let edge_rotation = edge_normal * edge_angle;
    let carrier_rotation = carrier_normal * carrier_angle;
    // Signed rotation distinguishes the two semicircles with the same endpoints,
    // and does not depend on a curve's chosen plane-normal orientation.
    (edge.front().point().distance(start) <= EPS
        && edge.back().point().distance(end) <= EPS
        && (edge_rotation - carrier_rotation).magnitude() <= 1e-9)
        || (edge.back().point().distance(start) <= EPS
            && edge.front().point().distance(end) <= EPS
            && (edge_rotation + carrier_rotation).magnitude() <= 1e-9)
}

fn same_ring(a: &Rim, b: &Rim) -> bool {
    a.cap.id() == b.cap.id()
        && a.center.distance(b.center) <= EPS
        && (a.radius - b.radius).abs() <= EPS
}

/// Subdivision narrows positive rational control hulls without changing the
/// authoritative carrier. Exhausted proof budgets reject the operation.
fn curve_separated(curve: &Curve, rim: &Rim, work: &mut usize) -> Result<bool> {
    let Some(curve) = crate::native_bounds::subdivision_curve(curve)? else {
        let enclosure = crate::native_bounds::curve(curve)?;
        return separated(enclosure.points(), rim, work);
    };
    let mut stack = vec![(curve, 0usize)];
    while let Some((curve, depth)) = stack.pop() {
        *work = work.saturating_add(curve.count().saturating_mul(8));
        if *work > MAX_PROOF_WORK {
            return Err(unsupported(
                "The collision proof exceeds its rational control budget",
            ));
        }
        if separated(curve.enclosure()?.points(), rim, work)? {
            continue;
        }
        if depth == 12 {
            return Ok(false);
        }
        let Some([a, b]) = curve.bisect(work, MAX_PROOF_WORK)? else {
            return Ok(false);
        };
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}

fn face_separated(face: &Face, rim: &Rim, work: &mut usize) -> Result<bool> {
    let enclosure = crate::native_bounds::face(face)?;
    *work = work.saturating_add(enclosure.len());
    if *work > MAX_PROOF_WORK {
        return Err(unsupported(
            "The collision proof exceeds its rational control budget",
        ));
    }
    // This enclosure uses the actual rounded rails, rather than a nominal
    // depth product which can fail to include the constructed transition.
    if separated(enclosure.points(), rim, work)? {
        return Ok(true);
    }
    // A plane's untrimmed parameter rectangle can contain far more than its
    // bounded face. Keep the exact face hull above as its conservative proof.
    if crate::planar::boundary_plane(face)?.is_some() {
        return Ok(false);
    }
    let Some(surface) = crate::native_bounds::subdivision_surface(face)? else {
        return Ok(false);
    };
    let mut stack = vec![(surface, 0usize)];
    while let Some((surface, depth)) = stack.pop() {
        let count = surface.count();
        *work = work.saturating_add(count.saturating_mul(8));
        if count > MAX_CONTROLS || *work > MAX_PROOF_WORK {
            return Err(unsupported(
                "The collision proof exceeds its rational control budget",
            ));
        }
        if separated(surface.enclosure()?.points(), rim, work)? {
            continue;
        }
        if depth == 14 {
            return Ok(false);
        }
        let Some([a, b]) = surface.bisect(depth % 2, work, MAX_PROOF_WORK)? else {
            return Ok(false);
        };
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}
fn band_separated(source: &Rim, other: &Rim, work: &mut usize) -> Result<bool> {
    let mut stack = interval_groups(source)?
        .into_iter()
        .map(|group| (group, 0usize))
        .collect::<Vec<_>>();
    while let Some((group, depth)) = stack.pop() {
        let hull = group_hull(&group, work)?;
        if separated(&hull, other, work)? {
            continue;
        }
        if depth == 12 {
            return Ok(false);
        }
        let Some([a, b]) = bisect_group(group, work)? else {
            return Ok(false);
        };
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}

fn collision_proof(
    solid: &Solid,
    rims: &[Rim],
    terminals: &circular_terminals::Terminations,
    work: &mut usize,
) -> Result<()> {
    for other in rims {
        for source in rims.iter().filter(|source| !same_ring(source, other)) {
            if !band_separated(source, other, work)? {
                return Err(invalid(
                    "Selected circular treatment volumes meet or cannot be separated; reduce their sizes",
                ));
            }
        }
    }
    for (index, rim) in rims.iter().enumerate() {
        // Only this rim's adjacent wall patches can be omitted: another
        // selected wall can still collide with this particular treatment.
        let own_walls = rims
            .iter()
            .filter(|other| same_ring(rim, other))
            .map(|other| other.wall)
            .collect::<HashSet<_>>();
        let mut own_edges = rims
            .iter()
            .filter(|other| same_ring(rim, other))
            .map(|other| other.edge.id())
            .collect::<HashSet<_>>();
        own_edges.extend(terminals.cap_edges(rim, rims));
        for face in solid.face_iter() {
            if own_walls.contains(&face.id()) {
                continue;
            }
            if face.id() == rim.cap.id() {
                for edge in face
                    .edge_iter()
                    .filter(|edge| !own_edges.contains(&edge.id()))
                {
                    if !curve_separated(&edge.curve(), rim, work)? {
                        return Err(unsupported(
                            "A circular setback may cross another cap boundary; reduce its size",
                        ));
                    }
                }
            } else if let Some(protected) = terminals.protected_edges(face, rim, rims) {
                for edge in protected {
                    if !curve_separated(&edge.curve(), rim, work)? {
                        return Err(unsupported(
                            "A circular terminal setback may meet another boundary on its radial face; reduce its size",
                        ));
                    }
                }
            } else if !face_separated(face, rim, work)? {
                return Err(unsupported(
                    "A circular setback may meet another face or cavity; reduce its size",
                ));
            }
        }
        for other in &rims[index + 1..] {
            if same_ring(rim, other) || rim.cap.id() != other.cap.id() {
                continue;
            }
            let a = rim.radius - rim.side * rim.radial;
            let b = other.radius - other.side * other.radial;
            let d = rim.center.distance(other.center);
            let preserved = if d > rim.radius + other.radius + EPS {
                d > a + b + EPS
            } else if d + other.radius < rim.radius - EPS {
                d + b < a - EPS
            } else if d + rim.radius < other.radius - EPS {
                d + a < b - EPS
            } else {
                false
            };
            if !preserved {
                return Err(invalid(
                    "Circular setbacks cross, touch, or change the source cap's containment order",
                ));
            }
        }
    }
    Ok(())
}

fn oriented(edge: &Edge, replacement: &Edge) -> Edge {
    if edge.orientation() {
        replacement.clone()
    } else {
        replacement.inverse()
    }
}
fn remap_wire(wire: &Wire, map: &HashMap<EdgeId, Edge>) -> Wire {
    wire.iter()
        .map(|edge| {
            map.get(&edge.id())
                .map_or_else(|| edge.clone(), |replacement| oriented(edge, replacement))
        })
        .collect()
}
fn preserve_surface(
    source: &Face,
    mut wires: Vec<Wire>,
) -> std::result::Result<Face, monstertruck::topology::errors::Error> {
    if !source.orientation() {
        for wire in &mut wires {
            wire.invert();
        }
    }
    let mut face = Face::try_new(wires, source.surface())?;
    if !source.orientation() {
        face.invert();
    }
    Ok(face)
}
fn replace_curve_in_frame(
    edge: &Edge,
    vertices: &HashMap<VertexId, Vertex>,
    curve: Curve,
    forward: bool,
) -> Result<Edge> {
    let a = edge.absolute_front();
    let b = edge.absolute_back();
    let a = vertices.get(&a.id()).unwrap_or(a);
    let b = vertices.get(&b.id()).unwrap_or(b);
    let edge = Edge::try_new(
        if forward { a } else { b },
        if forward { b } else { a },
        curve,
    )
    .map_err(|_| invalid("A setback collapsed a boundary edge"))?;
    Ok(if forward { edge } else { edge.inverse() })
}
fn assign_vertex(map: &mut HashMap<VertexId, Vertex>, source: &Vertex, p: Point3) -> Result<()> {
    if let Some(existing) = map.get(&source.id()) {
        if existing.point().distance(p) > EPS {
            return Err(invalid(
                "Selected circular setbacks require incompatible shared vertices",
            ));
        }
    } else {
        map.insert(source.id(), Vertex::new(p));
    }
    Ok(())
}

pub(crate) fn chamfer(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    construct(solid, selected, spec, Treatment::Chamfer, history)
}

/// Try complete native constructors before decomposing independent selections.
/// No failed candidate, partial component sequence or diagnostic mutates the
/// caller's construction history.
pub(crate) fn construct(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Err(invalid("Select at least one edge"));
    }
    if selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES
        || solid.face_iter().count() > MAX_FACES
    {
        return Err(unsupported(
            "The selected body exceeds the edge treatment complexity limit",
        ));
    }
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let source_edges = solid
        .edge_iter()
        .filter(|edge| selected.contains(&edge.id()))
        .map(|edge| (edge.id(), edge))
        .collect::<HashMap<_, _>>();
    if source_edges.len() != selected.len() {
        return Err(invalid(
            "A selected edge no longer belongs to the source body",
        ));
    }
    if let Some(first) = spec.first_face {
        let face = solid
            .face_iter()
            .find(|face| face.id() == first)
            .ok_or_else(|| invalid("Distance 1 face no longer belongs to the body"))?;
        if !selected
            .iter()
            .all(|id| face.edge_iter().any(|edge| edge.id() == *id))
        {
            return Err(invalid("Distance 1 face must adjoin every selected edge"));
        }
    }
    let mut ordered = selected
        .iter()
        .map(|id| {
            Ok((
                crate::shell_history::closure(&[history.edge(*id)?])?.to_string(),
                *id,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let ordered = ordered.into_iter().map(|(_, id)| id).collect::<Vec<_>>();
    let original_diagnostic = match attempt(solid, &ordered, spec, treatment, history) {
        Ok(Some(output)) => return Ok(Some(output)),
        Ok(None) => None,
        Err(error @ CadError::Unsupported(_)) => Some(error),
        Err(error) => return Err(error),
    };
    let mut incident: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
    for edge in source_edges.values() {
        for vertex in [edge.front(), edge.back()] {
            incident.entry(vertex.id()).or_default().push(edge.id());
        }
    }
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for id in &ordered {
        if seen.contains(id) {
            continue;
        }
        let mut pending = vec![*id];
        let mut members = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            members.insert(id);
            let edge = &source_edges[&id];
            for vertex in [edge.front(), edge.back()] {
                pending.extend(
                    incident[&vertex.id()]
                        .iter()
                        .filter(|id| !seen.contains(id))
                        .copied(),
                );
            }
        }
        groups.push(
            ordered
                .iter()
                .filter(|id| members.contains(id))
                .copied()
                .collect::<Vec<_>>(),
        );
    }
    if groups.len() <= 1 {
        return original_diagnostic.map_or(Ok(None), Err);
    }
    let mut candidate = history.clone();
    let mut output = solid.clone();
    for (index, group) in groups.iter().enumerate() {
        let first_face = if let Some(original) = spec.first_face {
            let reference = history.face(original)?;
            let descendants = output
                .face_iter()
                .filter(|face| {
                    candidate
                        .face(face.id())
                        .is_ok_and(|current| current == reference)
                        && group
                            .iter()
                            .all(|id| face.edge_iter().any(|edge| edge.id() == *id))
                })
                .map(|face| face.id())
                .collect::<Vec<_>>();
            let [face] = descendants.as_slice() else {
                return Err(unsupported(
                    "Distance 1 face has no unique retained descendant for an independent edge group",
                ));
            };
            Some(*face)
        } else {
            None
        };
        let current_spec = ChamferSpec {
            distance: spec.distance,
            second_distance: spec.second_distance,
            angle: spec.angle,
            flip: spec.flip,
            first_face,
        };
        let Some(next) = attempt(&output, group, &current_spec, treatment, &mut candidate)? else {
            return Err(original_diagnostic.unwrap_or_else(|| unsupported("An independent selected edge group has no complete native treatment constructor")));
        };
        let remaining = groups
            .iter()
            .skip(index + 1)
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        let surviving = next
            .edge_iter()
            .map(|edge| edge.id())
            .collect::<HashSet<_>>();
        if !remaining.is_subset(&surviving) {
            return Err(invalid(
                "An earlier treatment changes a later selected edge; the groups require a simultaneous corner/intersection construction",
            ));
        }
        output = next;
    }
    crate::validation::solid(&output)?;
    *history = candidate;
    Ok(Some(output))
}
fn attempt(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    let original = match attempt_native(solid, selected, spec, treatment, history) {
        Ok(Some(output)) => return Ok(Some(output)),
        Ok(None) => None,
        Err(error @ CadError::Unsupported(_)) => Some(error),
        Err(error) => return Err(error),
    };
    let mut candidate = history.clone();
    let Some(prepared) =
        straight_chains::prepare(solid, selected, spec.first_face, &mut candidate)?
    else {
        return original.map_or(Ok(None), Err);
    };
    let prepared_spec = ChamferSpec {
        first_face: prepared.first_face,
        distance: spec.distance,
        second_distance: spec.second_distance,
        angle: spec.angle,
        flip: spec.flip,
    };
    let result = attempt_native(
        &prepared.solid,
        &prepared.selected,
        &prepared_spec,
        treatment,
        &mut candidate,
    )?;
    if result.is_some() {
        *history = candidate;
    }
    Ok(result)
}

fn attempt_native(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    let mut diagnostic = None;
    for constructor in 0..4 {
        let mut candidate = history.clone();
        let result = match constructor {
            0 => concave_treatment::construct(solid, selected, spec, treatment, &mut candidate),
            1 => local_blend::construct(solid, selected, spec, treatment, &mut candidate),
            2 => circular_with_caps(solid, selected, spec, treatment, &mut candidate),
            _ => match treatment {
                Treatment::Chamfer => crate::planar::chamfer(solid, selected, spec, &mut candidate),
                Treatment::Fillet => {
                    crate::edge_round::round(solid, selected, spec.distance, &mut candidate)
                }
            },
        };
        match result {
            Ok(Some(output)) => {
                *history = candidate;
                return Ok(Some(output));
            }
            Ok(None) => {}
            Err(error @ CadError::Unsupported(_)) => {
                if diagnostic.is_none() {
                    diagnostic = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    diagnostic.map_or(Ok(None), Err)
}

fn circular_with_caps(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    let mut candidate = history.clone();
    let prepared = circular_caps::coalesce(solid, selected, &mut candidate)?;
    let current = prepared.as_ref().map_or(solid, |prepared| &prepared.solid);
    let current_spec = ChamferSpec {
        first_face: spec.first_face.map(|face| {
            prepared
                .as_ref()
                .and_then(|prepared| prepared.faces.get(&face).copied())
                .unwrap_or(face)
        }),
        distance: spec.distance,
        second_distance: spec.second_distance,
        angle: spec.angle,
        flip: spec.flip,
    };
    let result = circular(current, selected, &current_spec, treatment, &mut candidate)?;
    if result.is_some() {
        *history = candidate;
    }
    Ok(result)
}

fn circular(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Err(invalid("Select a complete circular rim"));
    }
    if solid.face_iter().count() > MAX_FACES
        || selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES
    {
        return Err(unsupported(
            "The selected body exceeds the circular chamfer complexity limit",
        ));
    }
    let selected: HashSet<_> = selected.iter().copied().collect();
    let mut source_edges = HashMap::new();
    for edge in solid.edge_iter() {
        source_edges.entry(edge.id()).or_insert_with(|| {
            if edge.orientation() {
                edge.clone()
            } else {
                edge.inverse()
            }
        });
    }
    for id in &selected {
        let Some(edge) = source_edges.get(id) else {
            return Err(invalid(
                "A selected edge no longer belongs to the source body",
            ));
        };
        match crate::bounded_carriers::circle(&edge.curve()) {
            Ok(_) => {}
            Err(CadError::Unsupported(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    let mut walls = HashMap::new();
    let mut rims = Vec::new();
    for edge in solid
        .edge_iter()
        .filter(|edge| selected.contains(&edge.id()))
    {
        if rims.iter().any(|rim: &Rim| rim.edge.id() == edge.id()) {
            continue;
        }
        let adjacent: Vec<_> = solid
            .face_iter()
            .filter(|face| {
                face.edge_iter()
                    .any(|candidate| candidate.id() == edge.id())
            })
            .collect();
        if adjacent.len() != 2 {
            return Err(invalid(
                "A circular rim must adjoin exactly two oriented faces",
            ));
        }
        let planes = adjacent
            .iter()
            .map(|face| crate::planar::boundary_plane(face))
            .collect::<Result<Vec<_>>>()?;
        let Some(cap_index) = planes.iter().position(Option::is_some) else {
            return Ok(None);
        };
        if planes[1 - cap_index].is_some() {
            return Ok(None);
        }
        let cap = adjacent[cap_index];
        let wall_face = adjacent[1 - cap_index];
        let cap_plane = planes[cap_index]
            .as_ref()
            .ok_or_else(|| invalid("The selected planar cap is absent"))?;
        let wire = cap
            .boundaries()
            .into_iter()
            .find(|wire| wire.iter().any(|candidate| candidate.id() == edge.id()))
            .ok_or_else(|| invalid("The selected edge is absent from its cap boundary"))?;
        // A selected circular chain may terminate on a real radial plane.
        // Its native endpoint incidence and terminal trims are established
        // together below; an arbitrary stop in a smooth wall is not admitted.
        let edge = source_edges[&edge.id()].clone();
        let ((center, normal, radius), _) = crate::bounded_carriers::circle(&edge.curve())?;
        if normal.dot(cap_plane.normal).abs() < 1. - 1e-10 {
            return Err(unsupported(
                "The rim is not perpendicular to its planar cap",
            ));
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = walls.entry(wall_face.id()) {
            slot.insert(carrier(wall_face)?);
        }
        let wall = walls
            .get_mut(&wall_face.id())
            .ok_or_else(|| invalid("The turned wall was not classified"))?;
        let column = wall
            .rims
            .iter()
            .position(|candidate| candidate.id() == edge.id())
            .ok_or_else(|| unsupported("The selected circle is not a natural turned rim"))?;
        let axial_extent = (wall.ends[1 - column] - center).dot(cap_plane.normal);
        if axial_extent.abs() <= EPS {
            return Err(unsupported(
                "The turned wall has no axial extent at this rim",
            ));
        }
        let axial_side = axial_extent.signum();
        let curve = edge.curve();
        let (a, b) = curve.range_tuple();
        let middle = curve.subs((a + b) * 0.5);
        let radial_direction = (middle - center).normalize();
        let surface = wall_face.oriented_surface();
        let uv = surface
            .search_parameter(middle, None, 100)
            .ok_or_else(|| unsupported("The rim has no verified parameter on its turned wall"))?;
        let slope = (wall.radii[1 - column] - radius) / axial_extent;
        let slant = (1. + slope * slope).sqrt();
        let expected = (radial_direction - cap_plane.normal * slope) / slant;
        let outward = surface.normal(uv.0, uv.1).normalize().dot(expected);
        if !outward.is_finite() || outward.abs() < 1. - 1e-9 {
            return Err(unsupported(
                "The wall orientation does not prove a regular inner or outer turned surface",
            ));
        }
        let occurrence = wire
            .iter()
            .find(|member| member.id() == edge.id())
            .ok_or_else(|| invalid("The circular cap occurrence is absent"))?;
        let ((_, oriented_normal, _), angle) =
            crate::bounded_carriers::circle(&occurrence.oriented_curve())?;
        let side = (angle * oriented_normal.dot(cap_plane.normal)).signum();
        if axial_side != -side * outward.signum() {
            return Err(invalid(
                "The circular cap traversal and turned material orientation disagree",
            ));
        }
        let (radial, depth, profile_weight) = match treatment {
            Treatment::Chamfer => {
                // Distance on the conical wall follows its exact generatrix.
                // The angle method uses the actual cap/wall normal angle.
                let cosine = -outward.signum() * slope / slant;
                let first_cap = match spec.first_face {
                    Some(id) if id == cap.id() => true,
                    Some(id) if id == wall_face.id() => false,
                    Some(_) => {
                        return Err(invalid("Distance 1 face must adjoin every selected rim"));
                    }
                    None => true,
                } != spec.flip;
                let second = if let Some(angle) = spec.angle {
                    let interior = (-cosine).clamp(-1., 1.).acos();
                    if !angle.is_finite() || angle <= 0. || angle >= std::f64::consts::PI - interior
                    {
                        return Err(invalid(
                            "The chamfer angle exceeds this conical material wedge",
                        ));
                    }
                    spec.distance * angle.sin() / (interior + angle).sin()
                } else {
                    spec.second_distance.unwrap_or(spec.distance)
                };
                if !spec.distance.is_finite()
                    || spec.distance <= EPS
                    || !second.is_finite()
                    || second <= EPS
                {
                    return Err(invalid(
                        "Both setbacks must be finite and exceed modeling tolerance",
                    ));
                }
                let (radial, along) = if first_cap {
                    (spec.distance, second)
                } else {
                    (second, spec.distance)
                };
                (radial, along / slant, 1.)
            }
            Treatment::Fillet => {
                let r = spec.distance;
                let cosine = -outward.signum() * slope / slant;
                if !r.is_finite() || r <= EPS || 1. + cosine <= 1e-12 {
                    return Err(invalid(
                        "A finite fillet radius requires a nondegenerate material wedge",
                    ));
                }
                (
                    r / (slant - outward.signum() * slope),
                    r * (1. + outward.signum() * slope / slant),
                    ((1. + cosine) * 0.5).sqrt(),
                )
            }
        };
        let wall_radius = radius + slope * axial_side * depth;
        if !wall_radius.is_finite()
            || wall_radius <= EPS
            || !radial.is_finite()
            || !depth.is_finite()
            || depth <= EPS
        {
            return Err(invalid(
                "The setback reaches a cone apex or exceeds finite geometry",
            ));
        }
        if radius - side * radial <= EPS {
            return Err(invalid("The radial setback consumes the circular cap"));
        }
        wall.offsets[column] = cap_plane.normal * (axial_side * depth);
        // Convert physical generatrix fraction to the homogeneous axial
        // parameter. The source wall is retained; this only chooses its trim.
        let fraction = depth / axial_extent.abs();
        let fraction = if column == 0 { fraction } else { 1. - fraction };
        let weights = &wall.net.control_points()[0];
        let ratio = weights[1].w / weights[0].w;
        let cut = fraction / (ratio + fraction * (1. - ratio));
        if !cut.is_finite() || cut <= 0. || cut >= 1. {
            return Err(invalid(
                "The circular setback leaves the bounded wall parameter domain",
            ));
        }
        wall.cuts[column] = cut;
        rims.push(Rim {
            cap: cap.clone(),
            edge,
            center,
            axis: cap_plane.normal,
            radius,
            wall_radius,
            profile_weight,
            radial,
            depth,
            side,
            axial_side,
            wall: wall_face.id(),
            column,
            rails: None,
            bounds: None,
        });
    }
    let mut keyed = rims
        .into_iter()
        .map(|rim| {
            let key = crate::shell_history::closure(&[history.edge(rim.edge.id())?])?.to_string();
            Ok((key, rim))
        })
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rims = keyed.into_iter().map(|(_, rim)| rim).collect::<Vec<_>>();
    for (index, rim) in rims.iter().enumerate() {
        for other in rims
            .iter()
            .skip(index + 1)
            .filter(|other| same_ring(rim, other))
        {
            if (rim.radial - other.radial).abs() > EPS
                || (rim.depth - other.depth).abs() > EPS
                || (rim.wall_radius - other.wall_radius).abs() > EPS
                || (rim.profile_weight - other.profile_weight).abs() > 1e-10
                || rim.side != other.side
                || rim.axial_side != other.axial_side
            {
                return Err(invalid(
                    "A complete circular rim has incompatible adjacent wall slopes or treatment parameters",
                ));
            }
        }
    }
    for wall in walls.values() {
        let original = wall.ends[1] - wall.ends[0];
        let remaining = wall.ends[1] + wall.offsets[1] - wall.ends[0] - wall.offsets[0];
        if remaining.dot(original.normalize()) <= EPS || wall.cuts[0] >= wall.cuts[1] {
            return Err(invalid("Opposite chamfers meet or consume the turned wall"));
        }
    }
    let mut prepared_controls = 0usize;
    for rim in &mut rims {
        let rails = Rails::build(rim, &walls[&rim.wall], treatment, Vec::new())?;
        prepared_controls = prepared_controls
            .saturating_add(rails.count())
            .saturating_add(18);
        if prepared_controls > MAX_CONTROLS {
            return Err(unsupported(
                "Selected circular rails exceed their aggregate control budget",
            ));
        }
        rim.rails = Some(rails);
    }
    let mut plane_vertices = HashMap::new();
    let mut wall_vertices = HashMap::new();
    for rim in &rims {
        let rails = rim.rails()?;
        let (a, b) = rails.planar.range_tuple();
        for (at_back, vertex) in [rim.edge.absolute_front(), rim.edge.absolute_back()]
            .into_iter()
            .enumerate()
        {
            let t = if (at_back == 0) == rails.forward {
                a
            } else {
                b
            };
            assign_vertex(&mut plane_vertices, vertex, point(rails.planar.subs(t))?)?;
            assign_vertex(&mut wall_vertices, vertex, point(rails.turned.subs(t))?)?;
        }
    }
    let mut connector_curves = HashMap::new();
    for rim in &rims {
        for old in [rim.edge.absolute_front(), rim.edge.absolute_back()] {
            if connector_curves.contains_key(&old.id()) {
                continue;
            }
            let front = plane_vertices[&old.id()].point();
            let back = wall_vertices[&old.id()].point();
            let curve = match treatment {
                Treatment::Chamfer => Curve::Line(Line(front, back)),
                Treatment::Fillet => Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
                    KnotVector::bezier_knot(2),
                    vec![
                        front.to_homogeneous(),
                        old.point().to_homogeneous() * rim.profile_weight,
                        back.to_homogeneous(),
                    ],
                ))),
            };
            connector_curves.insert(old.id(), curve);
        }
    }
    for rim in &mut rims {
        let mut connectors = Vec::new();
        for old in [rim.edge.absolute_front(), rim.edge.absolute_back()] {
            connectors.push(connector_curves[&old.id()].clone());
            connectors.push(Curve::Line(Line(
                old.point(),
                plane_vertices[&old.id()].point(),
            )));
            connectors.push(Curve::Line(Line(
                old.point(),
                wall_vertices[&old.id()].point(),
            )));
        }
        rim.rails
            .as_mut()
            .ok_or_else(|| invalid("Circular rails were not prepared"))?
            .connectors = connectors;
    }
    let terminals =
        circular_terminals::Terminations::discover(solid, &rims, &plane_vertices, &wall_vertices)?;
    let mut proof_work = 0usize;
    for rim in &mut rims {
        rim.bounds = Some(band_bounds(rim, &mut proof_work)?);
    }
    collision_proof(solid, &rims, &terminals, &mut proof_work)?;
    let mut plane_edges = HashMap::new();
    let mut wall_edges = HashMap::new();
    let mut seam_edges = HashMap::new();
    for rim in &rims {
        let source = history.edge(rim.edge.id())?;
        let rails = rim.rails()?;
        let planar = rails.planar.clone();
        let turned = rails.turned.clone();
        let p = replace_curve_in_frame(
            &rim.edge,
            &plane_vertices,
            Curve::NurbsCurve(NurbsCurve::new(planar)),
            rails.forward,
        )?;
        let w = replace_curve_in_frame(
            &rim.edge,
            &wall_vertices,
            Curve::NurbsCurve(NurbsCurve::new(turned)),
            rails.forward,
        )?;
        history.construct_edge(
            p.id(),
            std::slice::from_ref(&source),
            serde_json::json!({"operation":treatment.name(),"constructor":"circular_planar_rail"}),
        )?;
        history.construct_edge(
            w.id(),
            std::slice::from_ref(&source),
            serde_json::json!({"operation":treatment.name(),"constructor":"circular_wall_rail"}),
        )?;
        plane_edges.insert(rim.edge.id(), p);
        wall_edges.insert(rim.edge.id(), w);
    }
    for wall in walls.values() {
        for edge in wall.face.edge_iter() {
            if wall.rims.iter().any(|rim| rim.id() == edge.id())
                || seam_edges.contains_key(&edge.id())
            {
                continue;
            }
            let source = source_edges[&edge.id()].clone();
            let a = source.absolute_front();
            let b = source.absolute_back();
            let start = wall_vertices.get(&a.id()).unwrap_or(a);
            let end = wall_vertices.get(&b.id()).unwrap_or(b);
            if start.id() == a.id() && end.id() == b.id() {
                continue;
            }
            let delta = b.point() - a.point();
            if (end.point() - start.point()).dot(delta) <= EPS * delta.magnitude() {
                return Err(invalid(
                    "Circular setbacks meet or consume a shared axial seam",
                ));
            }
            let replacement = builder::line(start, end);
            history.retain_edge(replacement.id(), source.id())?;
            seam_edges.insert(edge.id(), replacement);
        }
    }
    let radial_edges = terminals.radial_edges(&source_edges, &plane_vertices, history)?;
    let mut cap_edges = plane_edges.clone();
    cap_edges.extend(radial_edges.iter().map(|(id, edge)| (*id, edge.clone())));
    let mut output_faces = HashMap::new();
    for rim in &rims {
        if output_faces.contains_key(&rim.cap.id()) {
            continue;
        }
        let wires = rim
            .cap
            .boundaries()
            .iter()
            .map(|wire| remap_wire(wire, &cap_edges))
            .collect();
        let face = preserve_surface(&rim.cap, wires)
            .map_err(|_| invalid("The planar rim setback breaks its cap boundary"))?;
        history.retain_face(face.id(), rim.cap.id())?;
        output_faces.insert(rim.cap.id(), face);
    }
    for wall in walls.values() {
        let mut map = wall_edges.clone();
        map.extend(seam_edges.iter().map(|(id, edge)| (*id, edge.clone())));
        let wires = wall
            .face
            .boundaries()
            .iter()
            .map(|wire| remap_wire(wire, &map))
            .collect();
        let face = preserve_surface(&wall.face, wires)
            .map_err(|_| invalid("The axial setback breaks its turned wall boundary"))?;
        history.retain_face(face.id(), wall.face.id())?;
        output_faces.insert(wall.face.id(), face);
    }
    let mut connectors: HashMap<VertexId, Edge> = HashMap::new();
    let mut bevels: HashMap<FaceId, Vec<Face>> = HashMap::new();
    for rim in &rims {
        let old_oriented = rim
            .cap
            .edge_iter()
            .find(|edge| edge.id() == rim.edge.id())
            .ok_or_else(|| invalid("The circular edge is absent from its source cap"))?;
        let plane = oriented(&old_oriented, &plane_edges[&rim.edge.id()]);
        let wall = oriented(&old_oriented, &wall_edges[&rim.edge.id()]);
        for old in [old_oriented.front(), old_oriented.back()] {
            if let std::collections::hash_map::Entry::Vacant(slot) = connectors.entry(old.id()) {
                let front = &plane_vertices[&old.id()];
                let back = &wall_vertices[&old.id()];
                let connector = Edge::try_new(front, back, connector_curves[&old.id()].clone())
                    .map_err(|_| invalid("A circular meridian collapsed"))?;
                let mut incident: Vec<_> = rims
                    .iter()
                    .filter(|other| {
                        other.edge.front().id() == old.id() || other.edge.back().id() == old.id()
                    })
                    .map(|other| history.edge(other.edge.id()))
                    .collect::<Result<_>>()?;
                // Two semicircles have the same pair of rim-member sources
                // at both endpoints. The original axial seam distinguishes
                // those endpoints without using coordinates or transient IDs.
                let seams = source_edges
                    .values()
                    .filter(|edge| {
                        !selected.contains(&edge.id())
                            && (edge.front().id() == old.id() || edge.back().id() == old.id())
                    })
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?;
                if seams.is_empty() {
                    return Err(unsupported(
                        "A circular transition endpoint has no distinct source seam ancestry",
                    ));
                }
                incident.extend(seams);
                history.construct_edge(connector.id(), &incident, serde_json::json!({"operation":treatment.name(),"constructor":"circular_shared_seam"}))?;
                slot.insert(connector);
            }
        }
        let rails = rim.rails()?;
        let p = &rails.planar;
        let w = &rails.turned;
        let mut surface = match treatment {
            Treatment::Chamfer => {
                if p.knot_vector() != w.knot_vector()
                    || p.control_points().len() != w.control_points().len()
                {
                    return Err(invalid(
                        "Circular rails lost their exact native parameter correspondence",
                    ));
                }
                NurbsSurface::new(BsplineSurface::new(
                    (p.knot_vector().clone(), KnotVector::bezier_knot(1)),
                    p.control_points()
                        .iter()
                        .zip(w.control_points())
                        .map(|(a, b)| vec![*a, *b])
                        .collect(),
                ))
            }
            Treatment::Fillet => {
                let middle = rails
                    .middle
                    .as_ref()
                    .ok_or_else(|| invalid("A fillet lost its middle rail"))?;
                if p.knot_vector() != middle.knot_vector()
                    || p.knot_vector() != w.knot_vector()
                    || p.control_points().len() != middle.control_points().len()
                    || p.control_points().len() != w.control_points().len()
                {
                    return Err(invalid(
                        "Circular fillet rails lost their exact native parameter correspondence",
                    ));
                }
                let controls = p
                    .control_points()
                    .iter()
                    .zip(middle.control_points())
                    .zip(w.control_points())
                    .map(|((a, corner), b)| vec![*a, *corner, *b])
                    .collect();
                NurbsSurface::new(BsplineSurface::new(
                    (p.knot_vector().clone(), KnotVector::bezier_knot(2)),
                    controls,
                ))
            }
        };
        // Swap axes to orient the surface; reversing angular knots performs
        // rounded arithmetic and would change the already certified rails.
        if old_oriented.orientation() == rails.forward {
            surface.invert();
        }
        let wire = Wire::from(vec![
            plane.inverse(),
            connectors[&old_oriented.front().id()].clone(),
            wall,
            connectors[&old_oriented.back().id()].inverse(),
        ]);
        let face = Face::try_new(vec![wire], Surface::NurbsSurface(surface))
            .map_err(|_| invalid("The conical strip has inconsistent shared boundaries"))?;
        history.construct_face(face.id(), &[history.edge(rim.edge.id())?, history.face(rim.cap.id())?, history.face(rim.wall)?], serde_json::json!({"operation":treatment.name(),"constructor":match treatment { Treatment::Chamfer => "exact_conical_rim", Treatment::Fillet => "exact_toroidal_rim" },"column":rim.column}))?;
        bevels.entry(rim.cap.id()).or_default().push(face);
    }
    output_faces.extend(terminals.rebuild(&radial_edges, &seam_edges, &connectors, history)?);
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut out = Shell::new();
        for face in shell {
            out.push(
                output_faces
                    .get(&face.id())
                    .cloned()
                    .unwrap_or_else(|| face.clone()),
            );
            if let Some(strips) = bevels.remove(&face.id()) {
                out.extend(strips);
            }
        }
        shells.push(out);
    }
    let result = Solid::try_new(shells)
        .map_err(|_| invalid("Circular setback boundaries do not close the body"))?;
    crate::validation::solid(&result)?;
    Ok(Some(result))
}
