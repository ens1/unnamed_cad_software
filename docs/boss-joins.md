# Native face-attached boss joins

Boss-Extrude retains the authored sketch, start/end conditions, direction,
units, selected contours and body scope. The kernel first attempts an exact
planar-contact constructor, then uses the existing native volume Boolean for
arrangements outside that constructor. It does not offset either solid to
manufacture an overlap. The desktop still uses the shared evaluated draft,
validated OK, checkpoint Cancel and one-operation undo path.

## Supported planar contact

A boss can start on an exposed planar stock face in any world orientation. Its
contact cap must face opposite the stock face, occupy the outgoing side of the
plane, and be either strictly inset or have a matching complete outline.

- An inset boss can have line, circular or positive native rational boundaries
  whose complete control hull fits strictly inside a convex straight polygon
  or full circular stock outline. Full circular containment uses the exact
  bounded circular carrier, including circles made from multiple arcs. A boss
  can contain holes: its contact cap is removed, its outer contact becomes a
  hole in the supporting stock face, and its inner contacts retain stock face
  islands. All resulting walls and face trims share the actual native boundary
  edges.
- Complete-outline continuation removes both coincident contact faces and
  sews the tool walls to the stock's retained contact edges and vertices. Outer
  and inner contours must have a unique edge correspondence. Native straight
  and bounded circular edges can match independent parameter representations;
  other spline edges require the same exact rational representation. Matching
  circular arc endpoints, carrier and angular traversal are required.
- Existing stock holes are retained when bounds in the supporting face frame
  prove strict separation from the entire contact outline. Matching complete
  outlines can include matching holes.
- Separated bosses can attach to the same original face, including separate
  selected material contours. A stepped or hollow stock can have other material
  above the contact plane when every outgoing stock face has a positive native
  control hull strictly separated from the entire new tool hull. Five exact
  projection bounds (world axes and two face-plane axes) provide this
  conservative separation proof. Otherwise a whole-stock supporting half-space
  proves separation. The validated closed oriented source and the outgoing
  contact establish which side starts in free space; no sampled inside/outside
  decision supplies geometry authority.

Every constructed output passes closed/oriented B-rep, endpoint/carrier and
face/edge membership validation. Unchanged stock faces and edges retain their
source identities. Reattached tool walls and connectors retain their authored
tool roles. A trimmed supporting face is named from its original stock face and
the tool walls that actually bound that trim; it is not renamed by its center.
Merged complete-outline seam edges retain the original stock edge identities.
The constructor is an internal replaceable kernel implementation and adds no
vendor objects or transient native IDs to the source document.

## Current limits

The contact proof accepts native planar, B-spline and positive rational spline
surface hulls with finite coordinates within ±1e12 mm. Implicit intersection or
parameter-curve boundaries and analytic surfaces without a native positive
control hull are left to the general Boolean. Complete-outline sewing currently
requires equal edge partitions; a circle with a different arc phase or a line
outline with extra segmentation needs exact subdivision before this path can
sew it. Inset containment currently requires a convex polygon or full circle on
the stock face. Existing holes whose conservative bounds overlap the contact,
partial overhangs, edge/vertex tangencies, a boss crossing existing material,
and more general coplanar arrangements also use the native Boolean. Failure to
resolve that arrangement remains an explicit geometry error.

Each input is admitted before hull construction: at most 4,096 faces, 16,384
oriented edge uses and 262,144 native topology/control units. Source spline
degrees are bounded by the shared native control preflight. The contact output
has at most 8,192 faces, with constructor topology/control allocation bounded by
the admitted source boundary/control counts; no fitted carrier is materialized.
Checked admissions limit face/outline matching to two million pair predicates
and supporting-plane, containment, separation and island-boundary work to
64 million native predicates per join attempt. Ambiguous outline matches or
duplicate retained topology identities fail explicitly.

Evidence for this change is source-level geometric construction and the parent
integration's compile/lint results. No automated tests, probes or interactive
acceptance runs were performed; usability and runtime acceptance remain with
the user.
