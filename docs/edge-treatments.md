# Native edge treatments

The Monstertruck adapter constructs exact B-rep fillets and chamfers using
application-owned selection and construction-history contracts. The native
paths share vertices and edges before validating the complete result. No
sampled relay strips, fitted blend surfaces or display meshes supply modeling
authority.

## Geometry paths

- **Simultaneous convex planar stock:** constant-radius cylindrical edge rounds,
  spherical three-edge corners, and circular mitres at two-edge corners. The
  two remaining walls may be drafted when their dihedral angles to the common
  wall agree. Chamfers clip all selected supporting setbacks together, so
  adjacent bevels share their corner topology.
  Later round features may retain earlier curved trims on those planar walls:
  only the newly selected endpoint neighborhoods must still have three straight
  edges and three supporting planar faces. Complete native enclosures protect
  distant previous blends and every retained trim against the new removal
  regions; unchanged curves and faces preserve their existing topology.
- **Local planar wedges:** a straight convex or concave edge adjoining two
  planar walls and two planar terminal faces may be rounded or chamfered even
  when the rest of the body has pockets, curved walls or earlier treatments.
  Separate selected edges may share a neighbouring sharp edge; both endpoint
  setbacks are constructed together and must leave a positive remaining span.
  The two neighbouring edges at each terminal vertex must be straight, with
  exactly three incident faces. Connected selections at one vertex use the
  simultaneous corner path rather than sequential rounding.
- **Straight rounds ending on cylindrical walls:** a constant-radius round of
  a convex right-angle planar wedge can end on a native circular extrusion wall whose
  axis follows one supporting plane's normal. The fillet retains a cylindrical
  carrier and shares its actual intersection curve with the terminal wall.
  At an inward corner of the source sketch, the neighboring circular edge and
  cylindrical wall continue on their original analytic geometry before being
  trimmed. A straight-ended approximation would give a different shape here.
  The source terminal must be a complete four-edge cylinder patch. Outward
  intervals isolate the intersection branch, preserve end order and bound the
  changed material; other trims and bodies must remain separated. Native
  intersection curves retain branch halfspaces through serialization and
  transforms, and never substitute their search guides for geometry.
- **Segmented straight chains:** when selected collinear sketch or Boolean
  edges divide one planar wedge, an additional native candidate removes paired,
  oppositely oriented coplanar seams and joins the selected edges at proven
  valence-two vertices. All source curves must lie within the bounded combined
  straight carrier, and merged planar supports must have one closed native
  boundary. The existing simultaneous constructor then treats the complete
  chain and its real terminal corners. Combined edge and support ancestry is
  explicit; an authored Distance 1 face follows its merged and trimmed native
  descendant. Normalization is private to this operation's candidate and is
  installed only with a complete validated treatment.
- **Concave pocket and cavity chains:** a convex planar void is treated as the
  material dual of a convex solid. Blind pockets receive a temporary exact
  planar entry cap. The complete selected void edge network is treated with
  the simultaneous convex constructor, then inverted into the stock. A changed
  entry wire replaces the matching inner stock-cap wire. Enclosed voids are
  recognized from their actual inverted supporting halfspaces, independently
  of shell ordering. Disjoint pocket/cavity selections are handled in one
  transaction. A pocket entry rim itself is treated using its actual stock
  carriers, not its temporary void cap.
- **Complete circular cylinder/cone rims:** equal/two-distance and angle
  chamfers use exact conical strips; constant-radius fillets use exact rational
  toroidal patches. This covers exterior turned rims, hole mouths, boss feet
  and blind-hole floors, in both convex and concave material arrangements.
  Multiple rings and both ends of the same wall are reconstructed together.
  On a cone, the wall distance follows the generatrix; fillet setbacks and
  meridian weights use the actual plane/cone angle. Unselected curved faces,
  existing treatments, pockets and other boundary shells are preserved when
  their separation from the changed volume can be established.
  Both extruded rational walls and native line-revolution walls are admitted.
  Revolved planar cap sectors are coalesced along shared native edges into
  complete circular disks or annuli; surviving rim edges, vertices and explicit
  Distance 1 face references retain their source relationships.
- **Circular chains ending on radial planes:** partial cylinder/cone rims may
  terminate at actual three-face corners on planar faces through the turning
  axis. This includes circular and annular sketch sectors, half-round stock,
  and corresponding radial-ended recesses in convex or concave material
  arrangements. The original radial faces receive the same exact conical or
  toroidal meridians as the new treatment faces. Cap radial edges and wall
  generatrices are trimmed together, including selections at both ends of a
  wall or both ends of a shared radial edge. Native planar revolution sectors
  can coalesce into bounded concentric sectors as well as complete disks or
  annuli. A selection cannot stop arbitrarily in an unselected smooth wall.

## Construction and validation

A rational turned wall must have positive weights, a clamped degree-one axial
parameter and corresponding circular end columns. The complete column
nets must prove a coaxial homothety; the bounded trim arcs must match endpoints
and signed angular traversal, and the two seams must be complete natural
straight generatrices. A single circle recognition or matching endpoints alone
is not enough. In particular, the two semicircles with identical endpoints
cannot be interchanged.
Native line-revolution walls recover the construction frame from their actual
bounded circular rims. Their retained analytic carrier is enclosed separately
using outward interval Rodrigues evaluation, including stored axis and trim
residuals. Neither path replaces the original wall surface.

The cap winding, wall normal and wall direction establish which side contains
material. Shared rails are transformed from the source rational curves.
Opposite-end trims must preserve axial order and positive radii. Radius or
setback values that consume a wall, reach a cone apex, collapse a cap or leave
incompatible seam vertices produce a diagnostic before a result is published.

Changed straight-edge material is bounded by exact triangular prisms. Changed
turned-rim material is bounded by rational annular bands. Positive rational
control hulls and outward interval knot subdivision establish separation from
other boundaries; every dehomogenization, subdivision and separating projection
retains arithmetic uncertainty. Exhausted proof budgets reject uncertain
geometry. Earlier analytic
sphere, torus and bounded revolution faces also use certified native enclosures.
Directional sphere and torus support bounds retain the complete affine transform,
including nonuniform scale, so a rotated part does not rely on the excess corners
of a world-axis box. Line revolutions use their certified trimmed boundaries;
their nominal finite generator segment is never substituted for an enclosure.
Every selected arc is checked against other selected changed volumes, including
its actual constructed homogeneous rails and weighted fillet controls; one
representative arc cannot stand in for the rest of a ring. Retained turned walls
keep their original native surface and parameterization; only their boundary
wires change. The complete boundary/carrier validator admits the new trim, and
interval source-wall rails enclose the removed strip. A complete positive
tensor-control displacement bound accounts for axial weight differences in
the constructed profile. On a shared cap, the original disjoint/nested circle
relationship must be preserved. For void duality, unrelated shells and material
islands must be separated from the selected removal slabs in the original
convex void, including full spherical-corner bounds.

An open circular chain is admitted only when its native endpoint incidence
identifies the cap, turned wall and one radial terminal plane. Its planar and
wall rail endpoints must lie strictly within the original straight terminal
edges. The constructed meridian's complete native enclosure must remain in that
plane. Every other terminal boundary is protected by the same rational band
separation proof, and the assembled radial and axial edge spans must remain
positive after all selected setbacks. Existing terminal faces and trimmed
source edges retain their provenance. Unchanged supports are preserved; no
new plane is invented to close an arbitrary partial arc.

Complete selections are attempted together first. If no complete constructor
accepts the selection, independent groups may use different native constructors
within the same operation. Groups are defined by shared source vertices; a
connected corner is never split into sequential treatments. All later selected
edge identities must survive each preceding group, and explicit Distance 1
faces must have a unique retained descendant. Each constructor checks the
current body, including geometry created by earlier groups. Any unhandled group
or changed later target rejects the entire operation.

Every candidate uses a private construction-history snapshot. Only a complete,
closed, validated output installs that snapshot. Unsupported candidates and
failed operations leave source history unchanged. The document-level command
continues to commit one operation through the existing validation/undo path.

## Remaining limits

These paths do not constitute a general rolling-ball surface solver. Arbitrary
freeform or intersecting cylinder/spline edges, partial turned rims without
radial planar terminations, unequal
radius corner networks, variable-radius fillets, unequal-dihedral two-edge
transitions, and arbitrary mixed-carrier corner patches remain unsupported.
Other straight-edge terminations on curved faces need further
intersection/transition construction. Native proofs may
reject valid but inseparable control-hull cases rather than publish uncertain
geometry. Conical walls ending at an apex require a separate singular topology
constructor. Mixed construction families at a connected or geometrically interacting corner
still require a simultaneous transition constructor.
Straight-chain normalization does not merge multiply-connected planar support
unions or curved supports, and it does not remove unselected authored edges
along the chain. Oblique cylindrical ends, conical/freeform ends and combined
extensions of one terminal face remain additional work. STEP export of the new
branch-constrained intersection trims is explicitly unavailable until the
exporter can preserve their geometry and branch; native TCAD source remains
editable and saveable.

Compilation, formatting and strict linting are the available implementation
checks. No code tests, probes or manual/UI smoke tests were written or run;
interactive acceptance remains with the user.
