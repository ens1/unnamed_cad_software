# Native circular edge fillets

This document describes the simultaneous convex-stock constructor. The current
dispatcher also supports local concave/convex wedges, pocket and cavity chains,
and cylindrical/conical rims; see [Native edge treatments](edge-treatments.md)
for the complete set of paths and their remaining limitations.

TCAD constructs constant-radius round fillets on the body's actual B-rep. The
entire selected edge set is reconstructed in one operation, preserving the
original stock until all tangent rails and corner boundaries are available.
There is no sampled mesh, fitted relay-sphere surface, or successful unchanged
fallback for a failed selected edge.

The native constructor covers straight convex edges between planar supporting
walls of a closed convex polyhedral stock envelope. Positive B-spline and NURBS
controls can prove the same planar/straight geometry as explicit planes/lines;
the enum name alone does not determine admission. At each selected endpoint,
the stock envelope must have three incident edges and three incident walls.

* An isolated selected edge ends at its actual third planar wall. A perpendicular
  cap gets an exact circular arc; an oblique cap gets an exact rational ellipse.
* Two selected edges meeting at a corner share one exact rational end curve in
  their cylindrical intersection mitre plane. The remaining walls must have
  equal dihedral angles to their common wall. This includes drafted corners,
  connected box-edge sets and convex right-prism cap rim chains.
* Three selected edges at a corner end at the common radius-offset plane center
  and share exact circular seams with a native spherical corner face. The sphere
  parameter frame keeps its periodic seam and poles outside the authored normal
  cone. Oblique trihedral corners use the same exact offset-plane construction;
  ill-conditioned plane systems are refused.

All circular strips are exact rational quadratic carriers, ruled between their
exact terminal curves. Planar walls are trimmed onto those same shared topology
edges. Fully rounded corners use analytic sphere carriers. The source document
continues to store its edge references and radius expression, independently of
kernel carriers and rendering.

Through-holes and contained blind pockets on one connected source shell can
remain when their complete positive
curve/surface control hulls prove disjoint from every selected removal region.
The separation bounds use all actual positive strip controls and complete
analytic corner-ball bounds, with wall-specific penetration depths. Original
inner wires and pocket faces remain exact and retain their source ancestry.
This deliberately conservative proof can refuse a geometrically separated
pocket when its carrier bounds overlap or when it has no supported finite
positive control hull; it never fits replacement pocket geometry.

The constructor rejects radii that consume a selected edge, consume a
neighbouring sharp edge, overlap terminal trims, or reach another stock wall.
At spherical corners, the complete ball must remain behind every stock plane.
For an oblique wall, exact ball containment replaces a
control-hull test that could otherwise reject a valid circular arc. Curved
boundaries retain f64 geometric authority and all output shells undergo closed,
oriented B-rep membership validation before the modeling draft may be applied.
Terminal arcs smaller than the kernel's geometric precision are refused.

Trimmed sharp edges and retained walls preserve authored ancestry. Cylinders,
rails, cap arcs, mitre seams and spherical corners have explicit constructor
provenance derived from selected source edges and incident wall/corner closures;
locations and transient kernel IDs do not become persisted reference roles.

Limits per selected body: 256 selected edges, 2,048 source faces, 65,536 protected
pocket control points, and 8,000,000 control-point/selected-edge separation work
items. These bounds are independent of the operation's larger overall selection
limit. Unsupported input returns a diagnostic rather than an approximate blend.

Variable radii, unequal-dihedral two-edge transitions,
nontrivalent corners, general curved-edge rounds, and existing rounded exterior
stock without a closed straight/planar supporting envelope still need further
kernel work in this constructor. Concave chains and enclosed-void shells use
the separate void-duality path described in the dispatcher documentation. Later
fillets may use the local or circular constructors when their complete support
and separation proofs hold; arbitrary blended stock is not assumed supported.
Compilation, formatting and strict linting were performed. Interactive and
geometric acceptance remain with the user; no tests or smoke runs were added.
