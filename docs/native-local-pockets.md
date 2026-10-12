# Local native pocket and drill insertion — October 12, 2026

The retained solid-mount part has a merged boss, bore, a planar bevel
and a toroidal boss-rim fillet. Its two rectangular mounting pockets and four
mounting-hole positions lie away from those finished regions. Whole-body convex
stock and layered-prismatic recognizers cannot represent that final boundary.
Falling through to an unrelated generic intersection constructor discarded an
otherwise straightforward local modeling opportunity.

TCAD now has a bounded local cap constructor for these separated operations.
It edits native source caps and shares their opening wires with exact tool
walls. It retains every remote native face and original shell component,
including negative void boundaries. It does not flatten, refit or approximate
the toroidal fillet or beveled geometry.

## Admission and material proof

1. Identify planar native entry/exit faces with normals parallel to the authored
   cut axis. Project a complete section only onto a unique face that covers its
   full material footprint. Existing inner trims must be separated. The chosen
   oriented opening establishes the local material seed; disjoint remote faces
   alone never establish that a tool lies inside stock.
2. Crop extrusion caps or native drill stations at the actual covered openings.
   Drill radius interpolation, inward shoulders and blind/through-all endings
   use the existing station contract. Torus/intersection stock extents for hole
   Through All now use outward native carrier projections.
3. Bound **both the original and cropped tools** through complete native
   enclosures. Positive rational control hulls and analytic affine support
   bounds supply these enclosures. Strict projection separation must exclude
   every untouched stock face and void. Original-tool clearance is included so
   cropping cannot silently skip an overhang or another material interval.
4. Replace covered caps using the actual tool station wires, invert deposited
   tool walls into subtractive boundaries, retain remote faces unchanged and
   validate the resulting closed native solid. All shell components remain
   boundaries of one stock container; a void is not returned as a positive body.
   The Boolean route explicitly requires this one-container invariant.

Planarity, complete cap containment and projection work share a bounded native
proof budget. Coverage charges the complete carrier/curve controls and outer
loop containment product before evaluating it. A hole array shares the budget
across all of its local drills. Uncertain/unbounded geometry does not become
material through tolerance widening or sampled predicates.

## Routing and identities

Existing convex insertion stays first. The layered-prismatic admission helper
uses the existing native recognizer and canonical axis; admitted stock/tool
pairs proceed through their established constructor and provenance grammar.
The local path handles stock outside that grammar, including distant bevels and
blends. Contextual Boolean differences likewise retain the layered constructor
first, then local cap insertion, then the existing general native boundary.

Sketch cuts publish traced cropped tools and source-cap ancestry through the
existing extrusion history path. Drills retain stable position identities and
their existing cropped station/tool return contract. Generic local Booleans
explicitly record the source cap and adjacent native tool-wall contributors.
No rendered mesh, face enumeration number or temporary kernel identity becomes
saved design intent. The backend fingerprint invalidates derived caches; native
source definitions and saved review models remain unchanged.

## Practical limits and evidence

The new path requires a covered planar opening and strict separation from all
other native boundaries. It does not establish arbitrary interacting curved
Booleans, unproved freeform envelopes, edge-breaking pockets, crossing existing
voids or distributing an opening across several stock faces. Tools containing
inner cap contours retain their existing constructors. Completely internal
pockets require the previous complete-stock proof rather than an invented local
material classification.

Static inspection identifies the saved rectangles at X = 30–50 / −50–−30 mm,
Y = −15–15 mm and Z = 8–12 mm as separated from the boss-rim carrier and front
bevel. This explains the intended native admission; it is not an executed
geometry result or interactive acceptance. Compilation and strict linting were
performed. No tests, geometry probes, manual smoke runs or UI checks were run.
