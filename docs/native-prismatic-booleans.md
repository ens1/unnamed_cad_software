# Native layered extrusion Booleans

TCAD adapter 26 adds a native analytic constructor for union, subtraction and
intersection of bodies bounded by parallel planar caps and common-axis straight
or circular extrusion walls. It is shared by the history-aware Extrude,
Revolve/Loft/Sweep/Rib combination paths, drilled-hole Boolean fallbacks and
explicit Boolean body operations.
Recognition is geometric; feature names alone do not select this constructor.

A planar support locates the sketch and supplies its normal. Its trim boundary
does not clip the sketch. The viewport and the worker use the same supporting
plane for native planes and for spline faces whose full control net is
coplanar within roundoff and the saved modeling tolerance. A named planar
start or end face also uses that plane beyond its trimmed boundary.

## Supported geometry

The operands may have different axial start/end heights, several height bands,
concave line/arc profiles, holes and disconnected boundary shells. This includes
parallel round bosses and holes overlapping straight or round extrusions, blind
and through cuts, and subsequent operations on retained layered results. Input
walls must remain complete native translation strips. Either end curve or
axial side can be split into several edges. Both end chains must cover the
same line or circular arc in order, with straight axial connections. Horizontal
faces may have several boundary wires. A previous result's intermediate wall
seam does not require a horizontal material cap at that height.

All planar section boundaries share one line/arc arrangement across every height
interval. Exact analytic crossings split the original native rational or linear
curves. Complete interval membership supplies set semantics; transitions between
adjacent heights produce the exposed horizontal caps. Native wall carriers,
shared cap edges, generator edges and vertices are retained or constructed once.
No mesh supplies the result geometry or material classification.

A sketch on a planar face may extend past its perimeter. A joined boss keeps
its overhang where it meets the stock over a positive area. A cut removes the
intersection with the scoped material. When the sketch crosses one stock edge
twice, both contact joins and layered cuts trim the middle interval in the
original edge's parameter space. A previous join may divide only the top edge
of a stock wall; that wall remains eligible for a later layered cut.
Through All uses native face enclosures projected along the extrusion direction,
including supported analytic blend surfaces. The planar boss join uses the same
enclosures to keep remote rounds or revolved faces on either body when they
are provably clear of the new contact.

Section membership uses a shared half-open ray rule at the actual native
vertices. Circular intervals are divided at analytic horizontal extrema, so
the rule does not compare independently rounded intersection angles to exact
zero/one endpoint parameters. Straight-ray orientation uses adaptive exact
determinants; circular branch membership uses an exact dyadic squared-distance
expansion instead of a rounded square-root hit. Unresolved finite arithmetic
returns a diagnostic. These predicates classify the partition; the original
native curves still construct every output edge.

The same planar loop constructor serves contact joins and layered Booleans.
Its errors now identify an operand section or an upper/lower output transition
and its height. A failed subtractive cap is no longer labeled a boss contact.

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
