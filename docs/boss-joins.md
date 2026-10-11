# Native planar-contact joins

Boss-Extrude retains the authored sketch, start/end conditions, direction,
units, selected contours and body scope. The kernel first attempts an exact
planar-contact constructor, then uses the native volume Boolean when that
constructor declines the arrangement. Ambiguous topology, exceeded budgets and
failed validation remain explicit errors. It does not offset either solid to
manufacture an overlap. The desktop still uses the shared evaluated draft,
validated OK, checkpoint Cancel and one-operation undo path.

The same contact constructor and construction-history recording serve additive
Revolve, Loft, Sweep and Rib through their shared combination path. Explicit
Boolean Union also tries contact joining in both operand orders. Each consumer
retains its own feature definition, tool construction and body scope; sharing
the join path does not expand which tools those features can construct.

## Supported planar contact

A boss can start on an exposed planar stock face in any world orientation. Its
contact cap must face opposite the stock face, occupy the outgoing side of the
plane, and overlap a material region of that face. Three native paths cover
strict inset, complete-outline continuation and line/arc contact arrangements.

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
  other spline edges require compatible native rational representations. Matching
  circular arc endpoints, carrier and angular traversal are required.
- The arrangement path handles partial overlaps and overhangs, nonconvex
  line/arc outlines, holes and different edge partitions. It intersects native
  lines and bounded circular arcs, including coincident carrier endpoints,
  resolves each split back onto its original curve and shares the resulting
  native edges across every incident face. Extra line segments or a different
  circular arc phase therefore do not require matching input partitions.
- Contact classification uses the complete outer and hole contours of both
  faces. The overlapping material contact is removed; exposed stock regions,
  tool underside regions beneath overhangs, holes and disconnected residual
  cap regions are retained with their correct orientation. Hole ownership is
  rebuilt from the resulting loops. This covers crossing hole boundaries when
  the line/arc arrangement resolves to closed, nonbranching regions. The inset
  fast path still requires strict separation from existing stock holes.
- Separated bosses can attach to the same original face, including separate
  selected material contours. A stepped or hollow stock can have other material
  above the contact plane when every outgoing stock face has a positive native
  control hull strictly separated from the entire new tool hull. Five native
  projection bounds (world axes and two face-plane axes) provide this
  conservative separation proof. Otherwise a whole-stock supporting half-space
  proves separation. The validated closed oriented source and the outgoing
  contact establish which side starts in free space; no sampled inside/outside
  decision supplies geometry authority.

Every constructed output passes closed/oriented B-rep, endpoint/carrier and
face/edge membership validation. Unchanged stock faces and edges retain their
source identities. Reattached tool walls and connectors retain their authored
tool roles. Trimmed contact faces carry their source-face ancestry and component
identity; split or shared contact intervals carry their source-edge ancestry
and interval identity. The inset path also records the tool walls bounding each
trim. These references do not use a face's center as its identity.
Merged complete-outline seam edges retain the original stock edge identities.
The constructor is an internal replaceable kernel implementation and adds no
vendor objects or transient native IDs to the source document.

## Current limits

The contact proof accepts native planar, B-spline and positive rational spline
surface hulls with finite coordinates within ±1e12 mm. Its contact tolerance is
1e-7 mm. The tool must have one boundary shell, occupy only the outgoing side
of its contact plane and have one unambiguous admissible stock-face/tool-cap
pair. Stock material elsewhere must pass the supporting-plane or local
clearance proof. This path does not resolve a tool crossing existing volume or
multiple simultaneous contact-face pairs.

The arrangement path requires complete bounded line/arc contours; arbitrary
spline intersections and implicit intersection/parameter curves are not
partitioned here. Native analytic surface families without the admitted
positive control hull also remain outside this contact constructor. General
spline outlines can still use the inset or compatible-outline fast paths.
Inset containment alone requires a convex polygon or full circle on the stock
face; line/arc arrangements remove that restriction when they resolve.
Edge-only contact, touching or branching result boundaries, unresolved
coincident intervals, ambiguous vertex matches and collapsed or numerically
uncertain regions are rejected or declined. The native Boolean fallback has
its own geometric limits; it is not a promise that every declined contact
arrangement succeeds.

Each input is admitted before hull construction: at most 4,096 faces, 16,384
oriented edge uses and 262,144 native topology/control units. Source spline
degrees are at most 32 under the shared native control preflight. The contact
output has at most 8,192 faces. The arrangement additionally limits split
boundary edges to 16,384 and cumulative split-carrier control cost to 262,144;
it does not fit replacement curves. Face/outline matching and cross-contour
intersection admission share a two-million-pair limit. Separate metered proof
counters for hull/containment work, native parameter resolution and vertex
matching have 64-million-work ceilings. These are individual counter limits,
not a single total runtime guarantee. Ambiguous outline matches or duplicate
retained topology identities fail explicitly.

Evidence for this change is source-level geometric construction and the parent
integration's compile/lint results. No automated tests, probes or interactive
acceptance runs were performed; usability and runtime acceptance remain with
the user.
