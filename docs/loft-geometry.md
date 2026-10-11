# Loft geometry and correspondence

Lofts retain their ordered, authored sketch sections. The adapter derives
correspondence from exact native curves; an equal sketch-member count, a shared
NURBS knot vector, or the order in which members were created is not required.
The same correspondence implementation serves solid and surface lofts.

Native line, B-spline and positive-weight rational curves are decomposed and
split at their actual parameter bounds. Every resulting piece retains its
original directed edge parameter interval and authored member ancestry. A
curve split changes its native representation without replacing its shape by a
polyline or fitting a curve through display samples.

Closed profiles use winding certified from actual native signed-area integrals.
Constant-weight spans integrate their Bernstein numerator directly;
positive-weight rational spans use bounded denominator/integral enclosures.
The first contour retains its native sense, including solid hole contours and
negative-axis lofts; later contours align to it. Uncertified or degenerate
orientation fails explicitly rather than trusting the control polygon.
Where their exact Bernstein angular
derivatives establish a visibility center, profiles align a geometric seam and
the union of their angular boundary partitions. This gives a circle and a
rectangle matching directions even though their native member counts and
rational weights differ. Nonradial closed profiles use a native support-extremum
seam. Bounded Bernstein derivative-root isolation finds candidate extrema on
the actual curves, and a native split retains the chosen point and ancestry.
Perimeter allocation refines native spans until their chord/control-polygon
length contrast is at most 0.5% or the geometric tolerance; midpoint interval
estimates choose correspondence. Long control handles therefore do not directly
inflate a whole member's allocation. These estimates choose section matching,
not replacement geometry or a guaranteed arc-length parametrization. Open
profiles align their endpoint direction and use the same bounded perimeter
refinement. Explicit solid-loft connectors retain their selected native boundary
vertices.

Corresponding rational boundaries are multiplied by each other's positive
denominator in the exact Bernstein basis. Both then have the same denominator,
so the skin interpolates Euclidean boundary positions rather than interpolating
unrelated projective weights. Uncontrolled loft skins use a degree-one section
direction directly, avoiding rounded cubic degree elevation. Explicit solid
endpoint influences retain their cubic carriers. Lines joined to circles retain
the exact line and circle sections. Adjacent strips share their native connector edges; adjoining
section pairs share the same refined intermediate section edges and vertices.

Surface Loft creates one open sheet from two through 256 ordered sections, each
containing one open or one closed material contour. All sections must agree on
open versus closed topology. Construction members are excluded. Surface Loft
requires distinct source sketches; repeating the same source section and closed
periodic lofts need additional occurrence and seam intent and fail explicitly.
Surface Loft
retains the specified section order, including reversals in plane height; it
does not silently reorder sections to satisfy a solid-volume assumption. Its
default interpolation between sections is ruled and position-continuous.
Guide curves, surface end-tangency controls and smooth interpolation through
multiple intermediate sections remain separate unsupported modes.

For parallel closed profiles, a positive angular-derivative certificate across
the complete rational section blend, together with nonzero signed plane
separation, proves each interpolated contour simple and each strip regular.
Either separation direction is permitted. This covers circle/polygon and
certified star-shaped concave profiles. Parallel nonradial surface sections
additionally support bounded native chart refinement. All section wires split
the same correspondence columns, retaining shared topology and directed authored
member intervals. Every accepted native chart has one fixed contour/height
projection whose actual rational Jacobian has positive principal minors;
off-axis derivatives are bounded rather than assumed zero by a parallelism
tolerance. This permits substantial shear and concavity without the former
positive-definite symmetric-Jacobian restriction. The certificate proves each
actual native face injective, not global separation between those faces.

Other surface skins use a bounded Bernstein certificate of an injective fixed
rational projection. Differentiable carriers use positive principal minors,
following the rectangular-domain [Gale–Nikaido theorem](https://link.springer.com/article/10.1007/BF01360282).
Position-continuous carriers with nonsmooth interior knots retain the stronger
positive-definite symmetric-Jacobian certificate. Proof coefficient subdivision
keeps the same projection axes and cancellation bounds across the complete face;
it never fits or samples replacement geometry. The generic proof supports
carrier degrees one through sixteen in each direction. Genuine constant-weight
carriers use their exact polynomial derivatives, avoiding unnecessary rational
degree growth and zero-denominator-derivative cancellation. Bernstein product
weights are reused within each proof product; proof budgets still bound the
complete operation. The specialized ruled
chart proof shares the source-degree-through-sixteen limit. An exhausted or
uncertain certificate remains an unsupported case; it is not proof that the
authored section is folded. Fallback diagnostics identify the section pair and
native strip.

Native manifold orientation, curve endpoints and curve-on-surface membership
are validated for the complete result. Patch regularity does not certify all
cross-patch or cross-section-pair self-intersections. In particular, a surface
that doubles back through previously used section heights can intersect itself
globally even when its individual strips are regular. Surface Loft is not a
claim of a watertight or globally embedded solid. Solid lofts retain their
additional section ordering, cap, contour-family and volume guards.

The exact adapter bounds each source curve to 4096 controls, each refined
contour to 4096 pieces, and all prepared section pieces to 32768. Complete
source sections admit at most 32768 native controls. Correspondence proof work
is bounded to 16 million units, and generic surface projection proof work to
32 million coefficient operations. Native area enclosure refinement is bounded
to sixteen levels and 4096 proof spans per contour. Support root isolation is bounded to 44
levels; native perimeter refinement to sixteen levels. Native chart refinement is bounded to twelve
levels and the shared piece budgets; scalar proof refinement to twelve levels
and 4096 visited cells per polynomial. Native weights must remain finite, positive and within the
recorded conditioning bound; near-coincident distinct partitions fail rather
than silently merging authored corners. Proof and
carrier limits are reported as unsupported geometry rather than substituting
rendering triangles. Surface wall identities include their two authored
section/member sources and refined parameter intervals. Section boundaries and
connectors likewise carry construction ancestry. Existing downstream references
to location-named faces or edges from older adapters can require explicit
replacement after cache invalidation; native sketch and feature definitions
are preserved.

Adapter 18 changes ruled carriers and proof-driven native surface partitioning,
invalidating earlier derived caches. Source schema and saved loft intent remain
unchanged. Additional native face splits can require downstream reference repair
when an old selection no longer identifies one complete face. Implementation
verification is compilation, formatting and linting. No code
tests, assertion probes, CLI geometry exercises or manual UI smoke tests are
part of this change. Interactive and geometric acceptance remains with the
user.

Final workspace formatting, strict linting and macOS build/package integration
passed on 2026-10-11 for adapter 18 / solver 11. The packaged app is
`dist/TCAD.app`; the saved complex loft has not been run as an acceptance check.
