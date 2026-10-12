# Native edge chamfers

This document describes the simultaneous planar-corner clipping constructor.
The current dispatcher also handles local concave/convex wedges, pocket/cavity
chains and complete cylindrical/conical rims; see
[Native edge treatments](edge-treatments.md) for current coverage and limits.

Chamfer evaluates the selected source edges together. Each edge contributes a
setback plane calculated from its two original supporting walls. Equal distance,
two distances, and distance plus angle use the same construction; Flip exchanges
the two wall distances. An explicitly selected Distance 1 face must adjoin every
selected edge. Without that face, wall ordering follows its outward normal and
is deterministic.

The native constructor handles convex straight edges between supporting planar
faces, including straight B-spline/NURBS curves and planar B-spline/NURBS carriers
when finite positive control bounds prove those properties. It does not require
the kernel to store those carriers as its Line and Plane enum variants.
Straight rational edges also require the existing Bernstein traversal proof to
exclude a curve that retraces the same line.

All setback planes refer to the original stock. Their half-spaces trim one shared
boundary representation, including adjacent selected edges at a common corner.
Coplanar setbacks from a split edge chain merge into one bevel with the complete
selected source closure. Faces share actual vertex and edge objects. Bevel caps
come from unmatched oriented shell edges, so existing edges on a later setback
plane cancel rather than becoming duplicate cap edges. No tessellated geometry
is used to construct the result.

An untouched curved pocket or an enclosed void survives when complete positive
control bounds prove it remains inside every setback. Inner wires stay attached
to their original planar carrier. A chamfer that intersects a curved pocket,
cuts a void, separates its source body, or splits a concave wall into disconnected
regions reports that limitation instead of manufacturing a shell from a mesh.

A merged boss can extend above the plate's top plane, making the whole body
non-convex while its outer plate corner remains convex. This constructor also
admits that bounded planar neighborhood: native edge winding and oriented
wall normals establish the convex material side, and each endpoint must have
two straight neighboring edges and a real third planar terminal face. The
complete local faces must remain behind those four source support planes.
Analytic setback/terminal intersections form the six vertices of a bounded
triangular removal prism. Outward interval projections must separate every
unrelated native face and void from that removal region. A distant boss or bore
may cross the infinite diagonal bevel plane; its original face is retained
without clipping when it is separated from the bounded removal prism.

Only the affected source faces and intersecting bevel faces are clipped in
this local mode. Trimmed source faces retain their authored identities through
successive planes. Previously created bevel caps participate in a later
adjoining corner, while a separated cap at a distant selected edge stays
unchanged. The same shared-shell boundary constructor closes the exact corner;
this is not a sequence of independently chamfered edges. The original global
supporting-stock path remains available for globally convex topology. Uncertain
separation, reentrant supports, non-planar terminal faces and consumed terminal
edges remain rejected. A shared four-million-work proof budget covers the whole
selection, including endpoint incidence, original carrier projections and later
bevel-cap separation; it does not reset for each edge. Admission is bounded to
512 selected edges and 4,096 source faces.

Native construction records retained and trimmed wall ancestry and constructs
new bevel references from the selected authored edges and incident wall
references. Later corner intersections use their exact wall adjacency. Kernel
pointer identities serve only as transient construction keys. Shortened known
source edges keep their explicit original ancestry, including segmented edges
whose pair of incident walls alone would be ambiguous. Every constructed
solid passes the ordinary closed-shell, edge/curve, and boundary/carrier
validation before it can become a validated modeling preview.

Distances are authoritative f64 millimeters and must exceed the constructor's
1e-7 mm boundary tolerance. Distance-angle must leave a positive finite second
setback for the actual material corner angle. Collapsed walls, branching corners,
reversed or disconnected bevel caps, and complete material removal produce
chamfer-specific diagnostics that advise reducing the setbacks.

This construction is not a general rolling bevel on curved carriers. The
dispatcher has separate concave and circular constructors; intersecting holes,
variable distances, partial-edge limits and arbitrary curved setback
intersection networks still require further exact construction work.
Compilation and linting are implementation checks; interactive geometric
acceptance remains with the user.
