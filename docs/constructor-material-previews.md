# Constructor material previews

The edge constructors can retain exact, local native material changes for
viewport feedback. These regions are disposable: they do not become part
features, body outputs, selection authority, exchange geometry or saved source.
The resulting model still passes its original native validation independently.

## Planar chamfers

Each proven planar setback clips native source wall polygons once. It retains
the positive discarded polygons using the same split vertices and native rails
as the model, then closes their boundary with the inverse of the actual bevel
cap. This constructs the finite removed region directly, including the changed
terminal faces, without subtracting nearly identical complete bodies.

Successive setback regions have disjoint interiors by construction. Opposing
planar contact patches are sewn through the existing bounded native contact
arrangement, removing shared interior interfaces. This supports adjoining edge
chamfer feedback when the contact exterior can be proven. The local constructor
limits still govern model acceptance. Display assembly additionally limits
regions to 64, faces to 4096 and contact attempts to 128.

A separate finite single-edge fallback constructs a removal prism from one
actual bevel quad, its two original planar support faces, and its two unique
planar terminal faces. This fallback is restricted to one complete straight
convex edge with perpendicular terminal planes. It does not imply support for
arbitrary chains or mixed-carrier corner junctions.

## Complete circular rims

Circular fillet and chamfer constructors retain their original rim, planar
trim rail, turned-wall trim rail and actual blend face. Shared native seam
edges close the annular material band. Convex rims publish removed material;
concave foot rims publish added material. Disconnected rings remain separate
validated local regions. The saved solid's complete four-segment boss rim has
the native carrier grammar addressed by this path; interactive acceptance is
pending.

This path requires complete circular rings with the constructor's proven native
planar/turned carrier neighborhoods. Partial rims requiring terminal closures,
arbitrary carrier transitions and unresolved corner junctions continue through
the existing bounded display comparison. Modeling does not acquire new blend
capabilities merely from a material-preview constructor.

## Cache and failure behavior

The adapter records both input and result native face identities and
orientations. Reuse requires both complete sets to match the actual comparison;
the owner reference alone cannot authorize reuse. New source builds clear the
records, while unchanged retained evaluations can reuse them. Each operation
refreshes its own record before constructing a new result.

The cache retains at most 128 operation records and 65,536 aggregate native
faces/source identity entries. Exhaustion clears older optional records. Display
mesh admission separately limits output to 256 regions, 4096 faces, 65,536
triangles and 196,608 points. No native preview objects are persisted or exposed
through application-owned source contracts.

Optional capture errors and panics are isolated from validated modeling. An
unproven closure or contact uses the existing bounded comparison; comparison
failure leaves neutral model geometry with visible feedback. Neither failure
can substitute approximate geometry for authority or enable Apply.

Implementation verification consists of compilation, formatting and strict
Clippy. No automated tests, geometry probes or interactive acceptance runs were
performed.
