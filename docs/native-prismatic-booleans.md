# Native layered extrusion Booleans

TCAD adapter 26 adds a native analytic constructor for union, subtraction and
intersection of bodies bounded by parallel planar caps and common-axis straight
or circular extrusion walls. It is shared by the history-aware Extrude,
Revolve/Loft/Sweep/Rib combination paths, drilled-hole Boolean fallbacks and
explicit Boolean body operations.
Recognition is geometric; feature names alone do not select this constructor.

## Supported geometry

The operands may have different axial start/end heights, several height bands,
concave line/arc profiles, holes and disconnected boundary shells. This includes
parallel round bosses and holes overlapping straight or round extrusions, blind
and through cuts, and subsequent operations on retained layered results. Input
walls must remain complete four-edge native translation strips; horizontal
faces may have several boundary wires. A previous result's intermediate wall
seam does not require a horizontal material cap at that height.

All planar section boundaries share one line/arc arrangement across every height
interval. Exact analytic crossings split the original native rational or linear
curves. Complete interval membership supplies set semantics; transitions between
adjacent heights produce the exposed horizontal caps. Native wall carriers,
shared cap edges, generator edges and vertices are retained or constructed once.
No mesh supplies the result geometry or material classification.

The checked arrangement uses adaptive exact line orientation and outward-rounded
intersection arithmetic. Uncertain near-parallel or near-tangent cases return a
specific diagnostic instead of silently dropping a possible crossing. Native
curve endpoint and inclusion checks still use the established 1e-7 mm B-rep
membership tolerance. This is a tolerance-qualified native constructor, not an
arbitrary-precision CAD kernel.

## Construction history

Layered reconstruction produces explicit identities for every wall, transition
cap, section interval and generator edge. Those identities contain one canonical source-face
construction table with compact descriptor indices, ordered intervals on the source coedges, source-plane
contributors and native endpoint roles. Native allocation IDs and current mesh
indices never enter saved identities. A combined cap does not inherit an
arbitrarily selected coplanar face's identity. Missing contributor ancestry and
excessive serialization work are reported before publication.

Legacy geometry-only callers retain their existing native backend; they cannot
publish untraced layered subdivisions. The current worker evaluation and normal
Apply/Cancel/undo transaction remain authoritative. Adapter 26 invalidates
previous derived caches.

## Limits and validation

This constructor does not solve arbitrary curved surface intersections. Oblique
or conical walls, nonconstant sections, general fillet networks, implicit
intersection trims and general freeform carriers continue through the other
native constructors or report their existing unsupported cases. Point/line-only
contacts that create nonmanifold boundaries, collapsed intervals and ambiguous
near-tangencies are rejected. Limits bound faces, levels, split intervals,
control counts, pair work and ancestry serialization.

Compilation, formatting and strict linting are source integration checks.
No code tests, runtime probes or interactive acceptance runs are authorized;
actual modeling acceptance remains with the user.
