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

Corresponding rational boundaries retain already identical synchronized
denominators, or are multiplied by each other's positive denominator in the exact Bernstein basis. Both then have the same denominator,
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
Surface Loft retains the specified section order and never silently reorders
sections. A two-section loft can run in either direction and can use angled
section planes when its complete native proof succeeds. Three or more sections retain exact ordered principal-plane slabs as a cheap
sufficient certificate. Other station sequences instead require complete native
separation between all skin faces across section pairs. This admits bounded
angled and turning sequences where the certificates succeed; it does not assume
that independent per-pair regularity prevents distant skins from intersecting.
Default interpolation between sections
is ruled and position-continuous.
Guide curves, surface end-tangency controls and smooth interpolation through
multiple intermediate sections remain separate unsupported modes.

For parallel closed profiles, a positive angular-derivative certificate across
the complete rational section blend, together with nonzero signed plane
separation, proves each interpolated contour simple and each strip regular.
Either separation direction is permitted. This covers circle/polygon and
certified star-shaped concave profiles. Ruled sections, including nonparallel source planes, additionally support bounded
native chart refinement. All section wires split the same correspondence columns,
retaining shared topology and directed authored member intervals. Every accepted
native chart has one fixed projected rational Jacobian with positive principal
minors. Parallel planes suggest a contour/height frame; other charts use dual
frames from native tangent and ruling directions. Candidate frames never establish
a proof by themselves: the complete common-denominator derivative coefficient
hulls must establish the three strict minor bounds. Off-axis derivatives remain
bounded rather than being assumed zero by a parallelism tolerance. This permits
substantial shear, concavity and angled section planes without the former
single-midpoint-frame restriction. The certificate proves each actual native
face injective; a separate native separation proof covers the rest of its skin.

Other surface skins first use the bounded fixed-projection certificate.
Differentiable carriers use positive principal minors, following the
rectangular-domain [Gale–Nikaido theorem](https://link.springer.com/article/10.1007/BF01360282).
Position-continuous carriers with nonsmooth knots retain the stronger
positive-definite symmetric-Jacobian certificate. A failure of this sufficient
condition does not establish folding.

A complete positive rational Bernstein carrier also has a local-atlas fallback.
Outward interval arithmetic refines the original homogeneous net with de
Casteljau subdivision. Every local rectangle must have a proven injective
projected Jacobian. All chart pairs then need either separated native hulls or
an injectivity certificate on their bounding native parameter rectangle, with
further bounded refinement when necessary. The latter permits contact at a
common parameter edge or vertex while ruling out different parameter points
occupying the same position. Rounded subpatches and rendering meshes are never
proof authority. This permits turning carriers which lack one valid projection
for the whole face, when global separation can still be established. The local
atlas currently requires one native Bernstein carrier; multi-knot carriers
retain the fixed-frame proof until their chart-continuity authority is expanded.
Both paths support native degrees one through sixteen and preserve explicit
conditioning/work limits. An unavailable or exhausted proof remains unsupported.

Nonradial ruled skins now require separation of all distinct native faces within
one section pair. A broad phase uses positive rational control hulls; remaining
pairs use directed-rounding interval arithmetic for every homogeneous de
Casteljau subdivision and projected hull comparison. Candidate axes only choose
separating directions. The complete native control enclosures prove the result.
Shared straight boundaries can establish opposite strict halfspaces; the
common native coefficient row supplies a Bernstein boundary factor. Omitting
that row requires exact projective equality, verified with product residuals.
Shared curved section boundaries can instead construct a continuous joint
native rational rectangle from exactly identical control rows. Its complete C0
monotonicity certificate proves separation of the two faces across that join.
Synchronized identical denominators are retained rather than unnecessarily
reweighted, making these joint carriers available without approximate matching.
Diagonal skin faces meeting only at one native vertex need exact projective
corner equality and strict opposite halfspaces for every other coefficient.
A shared topological identity or tolerance-only carrier membership does not
authorize any boundary or corner omission.
Away from that shared boundary, subdivision must prove disjointness. Intersections,
extra contacts and uncertain exhausted certificates return a diagnostic naming
the involved native strips, without substituting triangles or calling uncertainty
a demonstrated fold.

Uninfluenced solid lofts use the same native chart and complete skin separation
proofs for all outer/hole contours. Parallel sections retain their strictly
ordered height and known radial/affine/convex/nested-circle families. Other
multi-pair parallel sequences can use complete inter-skin separation instead of
the principal-plane slab restriction. Cubic endpoint-influenced solids retain
their existing supported-family and axial-monotonicity guards.

Nonparallel planar solid sections are admitted only with proven individual
ruled strips, complete skin separation, and native end-cap separation. Exact
floating-point expansions establish that every shared boundary coefficient is
on the cap's represented plane. All other skin coefficients must have a strict
interior halfspace sign, factoring the common boundary without an epsilon band.
This also separates the two caps. Merely tolerance-planar shared controls,
non-halfspace cap neighborhoods, and general trim collars remain unsupported.
The complete result still requires native closed manifold orientation, consistent
curve endpoints, and curve-on-surface validation. This is bounded admission of
additional valid families, not unrestricted freeform loft parity.

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
rendering triangles. Native skin separation is bounded to 4096 charts per skin,
131072 overlapping broad-phase face pairs, 65536 refinement cells per candidate
pair, 24 subdivision levels and 24 million operation-wide proof work units.
Local atlases admit at most 1024 charts and sixteen local subdivision levels;
chart-pair checks share the bounded native separation budgets. The multi-pair
slab fast path admits at most 262144 native section controls. Surface wall identities include their two authored
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

The current kernel pass adds nonparallel ruled-chart frames and complete
section-pair skin separation, and reuses both for concave uninfluenced solid
lofts. Source schemas remain unchanged; derived native chart partitions can
change, so downstream selections on former unsplit faces can require explicit
reference repair. Compilation, formatting and strict linting are the available
implementation evidence; no saved loft or runtime geometry probe was executed.

Adapter 26 final integration passed workspace compilation, formatting, strict
library/binary linting and the macOS package/signature checks on 2026-10-11.
This remains source and package evidence; no interactive or saved-model
acceptance exercise was performed.

The 2026-10-14 local-atlas and cross-skin extension changes evaluated carriers
and supported certificate paths. The integrated adapter fingerprint invalidates
old derived caches; saved loft intent and source identities remain unchanged.
Compilation and strict linting passed for the implementation. No tests, probes,
or interactive acceptance checks were performed.
