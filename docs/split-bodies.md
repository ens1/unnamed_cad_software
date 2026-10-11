# Split and Cut With Plane

Split contract 1 saves a scoped set of native solid outputs, an associative
splitting plane, a signed offset, normal reversal and retained side choice.
The definitions contain application-owned references and expressions, independent
of native kernel objects and display meshes. Split keeps both sides by default;
Cut With Plane opens the same editable feature with the negative side retained.
The resulting feature is named Split in the history.

Select a solid face, datum plane or named Front, Top or Right plane, then choose
Split or Cut With Plane. The docked property manager exposes the plane, offset,
normal reversal, Positive Side, Negative Side or Both Sides, and explicit solid
output scope. A planar face on a solid or surface supplies its complete supporting
plane; its finite trim boundaries do not crop the cut. Datum and coordinate-system
planes remain associative. Offset follows the unreversed source normal; reversal
changes the retained-side labels. Each scoped output contributes every native
solid it currently contains. Additional output scopes are explicit and editable.

Body and plane feature references belong to the current part definition. Editing
an assembly component uses Edit in Place and its local native references; picked
display-world coordinates do not become plane intent. Cross-component transformed
cutting references need further explicit occurrence-frame support. New source
solids continue to originate from sketches; finite kernel cutter boxes are internal
half-space construction tools, never user-created primitive features.

Both Sides retains the material on each side. A body wholly on one side remains
once, including tangency at an existing boundary. Keeping a side with no material
is a valid empty result, and an empty scoped source remains empty. Duplicate exact
native solids reached through different scope aliases are rejected. Other document
outputs remain outside this feature. Scoped outputs are consumed through the shared
branch reconciliation path and restored when the feature is suppressed or cancelled.

The feature uses the shared persistent-worker preview and source draft. Apply waits
for the current validated evaluation and commits one undo operation. Cancel restores
the starting source checkpoint. Saved parameters, body scope and construction
references participate in configurations, undo, library remapping and native saves.
Typed semantic quantities, source definitions and editing actions remain separate
from docked layout and styling. Interactive acceptance remains with the user.

## Exact native construction and bounds

Convex planar stock with explicit native line boundaries uses native
line/plane intersections and shared polygon-boundary reconstruction. The result
retains exact native planar carriers. Broader admitted stock uses native Boolean
difference against the opposite half-space and validates every retained B-rep.
This path handles supported curved/nonconvex bodies when their native carriers,
finite bounds, intersection construction and retained source ancestry are available.
Failed Booleans or inconsistent native boundaries remain explicit diagnostics.

The finite cutter's containment is established before geometry evaluation:

- Positive polynomial/rational controls bound finite native curves and surfaces.
- The native certified enclosure additionally covers its supported analytic
  spheres/tori, trimmed straight-generator revolution faces, parameter curves and
  implicit intersections through exact carrier/parameter enclosures. An implicit
  curve's approximate guide is never used as a stock bound.
- When that enclosure is unavailable, finite positive explicit control hulls can
  admit supported bounded spline-generator carriers. Uncertified analytically
  unbounded straight-generator trims are rejected.
- The enclosure is projected into the source plane frame. Its U/V limits and the
  far side of each cutter are enlarged by a finite margin based on model span,
  document tolerance and coordinate-scale rounding. The five artificial cutter
  walls lie strictly beyond the stock; only its authored plane can enter a retained
  result. Collapsed padding or nonfinite extents are rejected.

Native solid/surface geometry stays authoritative. Neither a mesh nor sampled
distance becomes a replacement boundary. The generic Boolean path currently
requires one connected native boundary per retained side. Disconnected/cavity
shell containment beyond that requires further exact classification. General curved
cutting surfaces, finite-face cropping, automatic saved-body part files and intersect
region selection remain additional work.

## Identity and admission bounds

Generated faces retain their logical positive/negative side, primary source
carrier, incident construction ancestry and canonical source-solid closure.
Edges retain their incident generated faces and any exact original edge ancestry.
Each side receives a separate identity-preserving native copy, so a cut through an
existing shared edge does not merge references across the two result bodies.
Centroids, display geometry, body ordinals and transient kernel IDs do not name
saved split topology. Missing source provenance, ambiguous carrier matching, or
disconnected faces/edges with indistinguishable source ancestry are rejected.

Source admission is bounded before hull conversion or copies: up to 256 scoped
outputs, 1024 solids, 4096 faces and 16384 edges in total; each solid has at most
512 faces, 2048 edges and 2048 vertices. Explicit native splines have degrees up
to 32, carrier nesting has at most 16 levels, and native-control preflight reserves
at most 262144 source units, including conservative analytic-conversion expansion.
Convex direct reconstruction is limited to 128 faces and 512 edges per solid.
Before result side copies, aggregate retained limits are 2048 solids, 8192 faces,
32768 edges and 524288 control units. A shared 64-million constructor/predicate
work budget and 32 MiB construction-reference budget reject oversized operations.
Stock, plane and cutter coordinates are finite and bounded to ±1e12 mm.
These are explicit current resource/geometry bounds, not claims of universal
Boolean robustness or acceptance based on compilation.
