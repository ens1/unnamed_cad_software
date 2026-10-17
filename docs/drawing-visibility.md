# Drawing line visibility

Drawing contract 11 and sheet-template contract 3 add a saved per-view
`view_display_modes` map. The choices are `wireframe`, `hidden_lines_removed`
and `hidden_lines_dashed`. Missing entries retain the wireframe interpretation
of existing documents and templates. New default sheets use hidden lines
removed. Sheet duplication, templates, undo, local saves and source/Git edits
carry the same choices. Removing a view removes its override.

The drawing editor exposes both a sheet-wide choice and an individual view
choice. Section-Z remains a sampled section intersection, with an explicit
separate fidelity description; whole-model visibility modes do not apply to it.
Visibility is separate from view placement, scale, dimensions and annotations.
Switching modes preserves the projected origin and uses the same conservative
surface/edge extents for fitting. Layout-only interaction avoids repeating
visibility computation; full production preview, worker validation and exports
use the saved visibility mode.

## Geometry contract

The classifier uses application-owned f64 mesh/curve and native-incidence
records. It does not depend on desktop renderer objects or Monstertruck types.
These are disposable results; authoritative source references are unchanged.
Dimensions still resolve their original native/sketch references and never
measure the displayed stroke or a visibility subdivision.

For a native planar face whose complete trim consists of native degree-one,
two-control rational curves, closed native wire/vertex incidence reconstructs
the polygon and any holes. Native vertex endpoints must agree with the source
curve endpoints within one eighth of the modeling tolerance; the reconstructed
vertices must also lie on the transformed native plane within that same budget.
The guide-recognition tolerance alone never authorizes this path. Ambiguous face identities, missing incidence, higher-degree or nonstraight boundaries
are not promoted to native polygons. Polygon parity and native plane depth
classify these faces, using the evaluation's modeling tolerance.

Other faces use their evaluated surface triangles. This includes curved faces
and planar faces with circular/general curved trim. Projected triangle domains
and interpolated depth classify their occlusion. Adjacent source-face queries
use the display chord tolerance to prevent a native edge from being spuriously
hidden by the chord error of its own surface. Other geometry can still occlude
that edge. Ghost components remain reference strokes and do not act as
occluders; hidden components are excluded.

The classifier splits rational curves in homogeneous Bézier form at projected
trim crossings and depth crossings. Curve coordinates remain rational curves;
classification does not replace them with screen samples. Numerical root
isolation uses a spatial-tolerance-derived parameter bound. Uncertain crossing
brackets stay visible. Poorly conditioned curves, invalid input, or exhausted
bounded work return an explicit error instead of silently accepting a partially
classified view. The supported alternative is a smaller displayed scope or an
explicit Wireframe mode.

Curved-face silhouette candidates come from facing transitions between adjacent
surface triangles inside the same native face. Ordinary internal tessellation
edges are not drawn. These silhouette candidates are themselves visibility
classified. They are sampled surface outlines, not native analytic silhouettes.

## Output fidelity and line styles

Each `ViewFidelity.visibility` result declares one of:

- `wireframe`;
- `native_planar`, with its numerical modeling tolerance;
- `tessellated`, with numerical tolerance and the evaluated display chord tolerance;
- `section_intersection`, with the display chord tolerance.

`native_planar` means native planar trimmed domains with bounded numerical
classification. It is not an interval-arithmetic proof of all floating-point
operations. A view with any sampled surface classification or sampled silhouette
is marked approximate. Unsupported source edge curves, sampled detail clipping
and repair clipping are also marked approximate. In particular, preserving an
exact rational edge carrier does not make curved-surface visibility exact.

Hidden strokes use distinct `HiddenPolyline` and `HiddenSpline` primitives and a
short 2 mm dash / 1 mm gap in paper units. Reference/center/ghost strokes retain
their separate long-short pattern. Paper preview, PDF and DXF share the same
classified primitives. DXF retains exact trimmed rational SPLINE entities (and
LINE entities for straight spans) with a registered `TCAD_HIDDEN` line type.
Detail/repair clipping retains its existing sampled-paper-curve behavior.

## Remaining scope and validation

This provides usable hidden-line modes, native planar-polygon visibility and
explicitly sampled visibility for ordinary curved geometry. Exact analytic
silhouettes and exact visibility against arbitrary trimmed curved surfaces
remain incomplete. A smaller display chord tolerance can refine sampled output;
it does not certify exact visibility. Tangencies, very narrow gaps, surface
trim chords and nearly grazing views can remain sensitive to that tolerance.
General exact sections/hatching and auxiliary/relative views are separate gaps.

Implementation evidence is source review, compilation, formatting and linting.
No code tests, assertion probes, manual smoke runs or interactive acceptance
were performed for this change. Production output still needs user acceptance
on representative models and external PDF/DXF consumers.
