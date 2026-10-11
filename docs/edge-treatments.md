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
- **Local planar wedges:** a straight convex or concave edge adjoining two
  planar walls and two planar terminal faces may be rounded or chamfered even
  when the rest of the body has pockets, curved walls or earlier treatments.
  Separate selected edges may share a neighbouring sharp edge; both endpoint
  setbacks are constructed together and must leave a positive remaining span.
  The two neighbouring edges at each terminal vertex must be straight, with
  exactly three incident faces. Connected selections at one vertex use the
  simultaneous corner path rather than sequential rounding.
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

## Construction and validation

A turned wall must have positive rational weights, a clamped degree-one axial
parameter and exactly corresponding circular end columns. The complete column
nets must prove a coaxial homothety; the bounded trim arcs must match endpoints
and signed angular traversal, and the two seams must be complete natural
straight generatrices. A single circle recognition or matching endpoints alone
is not enough. In particular, the two semicircles with identical endpoints
cannot be interchanged.

The cap winding, wall normal and wall direction establish which side contains
material. Shared rails are transformed from the source rational curves.
Opposite-end trims must preserve axial order and positive radii. Radius or
setback values that consume a wall, reach a cone apex, collapse a cap or leave
incompatible seam vertices produce a diagnostic before a result is published.

Changed straight-edge material is bounded by exact triangular prisms. Changed
turned-rim material is bounded by rational annular bands. Positive rational
control hulls and exact knot subdivision establish separation from other
boundaries; exhausted proof budgets reject uncertain geometry. Earlier analytic
sphere, torus and bounded revolution faces also use certified native enclosures.
Directional sphere and torus support bounds retain the complete affine transform,
including nonuniform scale, so a rotated part does not rely on the excess corners
of a world-axis box. Line revolutions use their certified trimmed boundaries;
their nominal finite generator segment is never substituted for an enclosure.
Every selected
ring is checked against other selected changed volumes, not merely against
unchanged source faces. On a shared cap, the original disjoint/nested circle
relationship must be preserved. For void duality, unrelated shells and material
islands must be separated from the selected removal slabs in the original
convex void, including full spherical-corner bounds.

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
freeform or intersecting cylinder/spline edges, partial turned rims, unequal
radius corner networks, variable-radius fillets, unequal-dihedral two-edge
transitions, and arbitrary mixed-carrier corner patches remain unsupported.
Local straight edges that terminate on curved edges or curved terminal faces
also need further intersection/transition construction. Native proofs may
reject valid but inseparable control-hull cases rather than publish uncertain
geometry. Conical walls ending at an apex require a separate singular topology
constructor. Mixed construction families at a connected or geometrically interacting corner
still require a simultaneous transition constructor.

Compilation, formatting and strict linting are the available implementation
checks. No code tests, probes or manual/UI smoke tests were written or run;
interactive acceptance remains with the user.
