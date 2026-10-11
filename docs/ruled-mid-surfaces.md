# Ruled Surface and Mid-Surface

Surface intent contract 3 saves application-owned `Ruled` and `MidSurface`
definitions. Source geometry, dimensions, dependencies and construction
references are independent of native kernel objects and viewport meshes. Both
commands use the docked feature editor, shared worker preview, current validated
Apply, checkpoint Cancel and one committed undo operation. These are source
capabilities; interactive acceptance remains with the user.

## Ruled Surface

Select a model edge, a native sketch member or a reusable reference curve, then
choose **Ruled Surface**. Add or replace sources in the property manager. Each
source record has a persistent ID and a curve reversal option. Extension uses an
explicit expression vector, associative axis, plane normal or two reference
points, with a signed length expression. Face orientation reversal is separate
from curve reversal and extension direction.

The kernel constructs the exact surface `C(u) + v * displacement` from explicit
native lines and positive rational B-spline/NURBS curves. Native knot insertion
splits each source into bounded Bezier spans; homogeneous controls are translated
without fitting. Circular/elliptic arcs and suitable spline spans therefore
retain their exact rational carriers. Each proven span is an independent surface
strip; adjacent strips can be joined with the existing Knit command.

Supported native degree is 1–8, with 1–256 source records, at most 4096 source
controls per curve and 4096 output strips. Singular, retracing, folded or
ill-conditioned cases without the regularity certificate below are rejected.
Analytic parameter curves, including nonzero-pitch helices, and implicit
intersection curves require additional exact extrusion support and are rejected.
There is no approximate guide-curve substitution. Variable-angle/tangent ruled
surfaces, radiated fans and automatic strip sewing remain further work.

## Mid-Surface

Choose **Mid-Surface** with a selected model face, or select the two face sources
in the property manager. The editor accepts faces of solids and surface bodies.
Each source records its actual output owner and construction-based face
reference. Position is an expression between 0 and 1: 0 retains the first source
position, 0.5 is the midpoint, and 1 retains the second. The source bodies remain
available. Result orientation follows the first face, with an explicit reversal.

For planar faces, the source planes must be parallel and distinctly separated.
The complete trims, including inner loops, must have a unique correspondence
after translating along their common normal. The result translates the complete
first face by the selected fraction of that normal separation, retaining every
matching explicit native line/B-spline/NURBS boundary. Implicit intersection and
analytic parameter trims require further exact correspondence support. Lateral
trim shifts, nonparallel source planes,
ambiguous loop correspondence and unmatched trims are rejected.

For rational faces, each source must be a proven full natural four-sided,
clamped, finite positive-weight patch without holes, existing trims, collapsed
sides or periodic seams. The second patch has explicit U/V swap and U and V
reversal controls. After that correspondence, the normalized knot bases and
control-grid dimensions must match, and normalized rational weights must be
exactly equal. The result is the exact mean of corresponding native U/V
positions, with at most 1024 source controls and degree 1–8 in each direction.
This construction supports suitable curved patches; it is a normal-distance
midpoint only for parallel planar inputs. Automatic surface pairing, general
trimmed rational patches, unequal-weight means, incompatible basis refinement,
periodic carriers and general equal-normal-distance mid-surfaces remain further
work.

## Validation and reference identity

Every rational output must pass a bounded Bernstein coefficient certificate.
The conservative f64 certificate uses one fixed projection frame chosen from native derivatives,
splits the complete carrier into native Bezier patches, and proves that the
symmetric part of its projected rational Jacobian is positive definite for
position-continuous carriers with nonsmooth knot joins. Differentiable carriers
instead prove all principal minors of that Jacobian positive throughout the
complete rectangular domain, following the [Gale–Nikaido theorem](https://link.springer.com/article/10.1007/BF01360282).
This admits valid shear that the stronger symmetric-Jacobian test rejected.
Interior knot multiplicity may not exceed the native degree: this retains
position continuity between proof patches. The weaker principal-minor proof is
used only when interior multiplicities are strictly below degree, preserving
differentiability across all joins.
Positive rational weights and that
certificate establish regularity and global injectivity. Derivative samples
choose the certificate frame; samples do not establish the result. There are at
most 256 native proof patches and eight million scalar coefficient operations per
certificate, plus 32 million coefficient operations across the complete operation.
Bounded Bernstein subdivision can prove positivity when an initial control hull
is inconclusive; it preserves the same projection frame and cancellation bounds
and admits at most twelve subdivision levels and 4096 cells per polynomial.
Two selections that resolve to the same native face are rejected, including
aliases through different body outputs. Coefficient majorants include product/subtraction cancellation;
positive signs require a conditioning margin relative to those majorants.
Numerical uncertainty or exhausted proof bounds reports an explicit
unsupported case. Separate strips/source bodies are not subjected to a general
surface-intersection or collision-trimming workflow.

Generated faces and edges carry source construction ancestry. Ruled strips use
their persistent source record, actual native source reference, normalized native
span and side constructor. Mid-surface faces use both face references, and their
boundaries use the corresponding pair of actual source edges. Face centroids,
tessellation, occurrence order and kernel object IDs do not name new topology.
Planar and rational correspondence checks prove eligibility; they do not replace
the authored source references. Native geometry comparison uses the existing
`1e-7 mm` boundary tolerance, with separate finite and conditioning guards.

Typed semantic quantities, source references, capabilities and owning-feature
actions are published independently of default docked controls and styling.
Saved source dependencies and expressions participate in rebuilding,
configurations and library identity remapping.
