# Native offsets, thickening and lateral shell openings

This document records the implemented offset extension and its current limits.
The source geometry remains the authoritative f64 native B-rep. Display meshes
do not supply offset curves, surfaces, collision decisions or references.

## Extruded sheets with tangent bends

Surface Offset and Thicken accept sheets made from complete planar and
cylindrical strips extruded along one common direction and axial interval.
Their cross-section may contain both lines and circular arcs, convex and concave
bends, and different bend radii. Adjacent strips must share native generator
edges and a continuous oriented normal. Each strip currently has one complete
four-edge boundary: two translated profile rails and two straight generators.

Each supporting plane moves along its normal. Each cylindrical carrier changes
radius with its oriented normal. Exact affine transformations preserve the
native rational curves and surfaces. Shared native vertices and edges remain
shared; every generated face and edge retains its source constructor mapping.
The existing surface history adapter records those mappings and the endpoint
ancestry of thickening connectors.

The complete cross-section collar is bounded by straight segments and circular
arcs. Pairwise boundary intersections and interior containment reject folds,
contacts away from an existing native seam, and collars which overlap. Offset
checks the whole interval from the source to the requested distance. Mid-plane
Thicken additionally checks the combined interval across both sides; two
independently acceptable one-sided offsets are insufficient.

Intersection and ray-classification calculations use outward interval bounds.
An unresolved root cannot be discarded as an intersection outside the bounded
span. Native endpoint contacts receive an exception only when their complete
root enclosure fits within that actual shared endpoint's positional tolerance.
Approximate infinite-line or circle coincidence never bypasses the intersection
calculation. A separate complete-span check handles the already identified
native seam. Near-coincident or poorly conditioned arrangements can consequently
remain unsupported even when they look plausible in the viewport.

The implementation bounds one mixed-carrier sheet to 512 patches, 4,096 native
controls per edge and 131,072 edge controls overall. The existing common-carrier
normal-field path remains available for plane/cylinder/cone/sphere/torus sheets
within its own carrier and trim restrictions. Complete translational rational
control nets are recognized directly as cylinders, avoiding an artificial cone
slope caused by separately recovered radii differing by a final floating-point
bit.

## Cylindrical and conical shell openings

The existing coaxial cylindrical/conical shell constructor now also accepts
complete lateral-wall openings, including combinations with end-cap openings.
All angular patches belonging to a chosen wall contour must be selected.
Removing only one angular patch remains unsupported because it needs additional
offset intersections at the new opening corners.

A removed end cap keeps its original plane. A removed lateral wall similarly
keeps its original cylindrical or conical carrier while retained cap and wall
offsets are intersected with it. The output opening is taken from the union
boundary of the retained native faces. A boundary shared only by removed faces
does not generate an extra wall or a zero-width rim.

Opening rims retain exact planar, cylindrical or conical geometry. The affine
radius functions are checked at both ends of the complete axial interval, which
excludes radial collapse and crossing of the supported inner and outer walls.
Generated rims reference their removed opening face and original boundary edge.
Each normal connector additionally records the original endpoint's incident
authored edge roles and start/end labels. This distinguishes the two connectors
of a two-semicircle opening without using coordinates or transient kernel IDs.

This constructor still requires two unsubdivided planar end caps and at most
one coaxial inner contour. Positive end-wall height and positive, properly
nested radii are required. Removing every face is invalid. Legitimately
disconnected retained wall components are returned as separate closed bodies.

## Remaining engineering work

These additions do not establish general NURBS offsets, sharp mixed-carrier
sheet joins, arbitrary trimmed bend sheets, partial angular shell openings,
arbitrary variable-thickness transition blends, or topology-changing collapse
and intersection trimming. Fully rounded equal-radius convex envelopes,
supported turned meridian shells and other existing specialized constructors
retain their separately documented contracts.

Verification follows the repository policy: compilation, formatting, static
review and linting. No code tests, assertion probes or interactive acceptance
checks were written or run. Compilation is not evidence of successful
interactive modeling; acceptance remains with the user.
