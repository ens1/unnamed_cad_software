# Connected analytic cap rims and uniform shells

This checkpoint expands actual native construction for a bounded but useful
mechanical family. It does not establish unrestricted mixed-carrier blend
networks or general NURBS shelling. Authoritative geometry remains f64 native
B-rep with explicit millimeter tolerances; meshes, sampled relay spheres and
fitted replacement curves are not construction inputs.

## Connected rim construction

`connected_rims::construct` accepts complete closed cap contours of a finite
straight extrusion whose native members are lines and circular arcs. Selected
members must join tangentially, with an oriented continuous side-wall normal.
The contour can contain convex and concave circular portions, circles/bores,
rounded slots and multiple independent selected contours. Other cap contours
may remain unselected and need not be tangent at their own corners.

The complete source stock is recognized, including the opposite cap and every
side wall. Each side must be a complete four-edge plane/cylinder translation
patch with a common finite axial interval and exact rational correspondence
between its two rails. This prevents unrelated pockets or trim fragments from
borrowing a two-dimensional proof. It currently requires unsubdivided native
side patches and admits at most 512 cap members.

One graph constructs the cap setback rail, axial wall rail, meridian seam and
trimmed generator for each native endpoint. Adjacent selected members share
the same native vertices and meridian edge. Each fillet strip is an exact
positive rational product of the original line/circular traversal and its
quarter-circle cross-section. Each chamfer strip is an exact ruled product.
The source rational denominator and knot vector remain common across every
rail; unequal weights or different parameterizations are not silently treated
as equivalent. All three-dimensional wall and cap replacements preserve
source references, and meridian ancestry includes both incident source members
and their endpoint roles.

Constant-radius fillets and equal-distance, two-distance and distance-angle
chamfers use this connected path. The selected first face and flip retain
their meaning; the perpendicular wedge requires an angle strictly between
zero and 90 degrees. Setbacks must exceed native resolution and leave a
positive remaining axial wall.

The shared `parallel_offset::certify_cap_collars` function bounds the complete
selected normal collars with exact line/circular spans and outward interval
intersection calculations. Only actual shared source vertices authorize a
complete tangent normal seam. Pairwise collar boundary intersections and
interior containment reject folds, overlap and remote contacts. Unselected
contours are protected exact spans and are checked against every collar.
This proof is reused by shelling rather than duplicated as another approximate
intersection predicate. Ill-conditioned arrangements may still be refused.

Closed exact removal collars supply added/removed-material preview data.
They reuse the same final blend surfaces, shared seams and original stock
walls; the preview does not overlay a second highlighted whole body. Failure
to build this optional display cache leaves it unavailable rather than
substituting geometry or bypassing the final body validation.

Existing planar corner, local wedge and turned-rim constructors remain active
for their separately supported families. Partial contour selections, sharp
selected contour corners, mixed junctions involving arbitrary intersecting
carriers, multiple opposing cap rounds in one operation, variable radii and
unresolved transition patches remain outside this new connected constructor.

## Uniform shell compatibility

`connected_shell::shell` recognizes finite stock with one cap carrying complete
constant-radius rounded contours, and an opposite planar cap. Each rounded
strip must match the complete native rational product, both quarter-circle
meridians, its tangent cap rail and its perpendicular side wall. Every wall
and opposite trim must match the recovered complete stock profile. A sampled
lookalike cannot borrow the certificate. A source may contain unrounded cap
contours alongside rounded ones; all recovered contours must support the
complete tangent normal collar needed by shelling.

Uniform inward and outward wall offsets preserve exact cylinder/torus blend
carriers. Rounded cap tangent rails move with the cap plane. Perpendicular wall
rails move with their side-wall normal. The sharp opposite cap is intersected
with the offset walls by reconstructing their finite translation patches,
instead of retaining an obsolete finite parameter domain. Actual source
vertices/edges key the shared reconstruction graph. Independent offset maps
must agree across complete rational curves before an edge is reused.

Either or both planar caps may be openings. Their source boundary loops are
paired with the exact reconstructed offset loops and closed by native ruled
rims. Closed inner and outer shells retain their separate material boundary
orientation. Connected wall components are returned as distinct bodies where
appropriate.

Complete profile collars bound the source, rounded cap setback and requested
wall displacement together. Positive blend radius, cylindrical/torus focal
limits, remaining axial wall extent and ordered cap planes guard the normal
map against collapse and crossing. Uniform thickness is required; variable
wall transitions and lateral openings need additional reconstruction and
remain unsupported. Inward thickness reaching a fillet's focal radius is
rejected explicitly rather than deleting that blend or inventing a patch.

## Evidence and limits

Implementation checks are compilation, formatting, static source review and
strict Clippy only. No code tests, assertion probes, runtime geometry checks
outside product validation, UI runs or manual smoke tests were performed.
Compilation is not interactive acceptance evidence. The final native B-rep
consistency gate still runs during normal feature evaluation.

The dependency fingerprint must advance for these construction changes.
Source operation definitions remain compatible; downstream references use
constructor ancestry, and geometry caches must rebuild under the new backend.
