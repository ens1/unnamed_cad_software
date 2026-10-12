# Viewport detail and overlay visibility

`tcad.display-mesh` version 1 separates disposable viewport tessellation from
modeling accuracy. Its request carries the exact evaluation/source/backend
identity, a separate display generation, source output identities and a display
tolerance in millimetres. It cannot validate a model, enable Apply, change
inspection properties, or supply exchange geometry.

The desktop estimates a local chord tolerance from the current camera, viewport
pixel resolution, nearby display bounds and an upper bound on occurrence scale.
Detail levels are quantized in half-octaves and requested after 180 ms without a
level change. The default target is 0.5 physical render pixels, bounded to
0.0001–0.1 mm. The target is a display-quality request, not a new dimensional
tolerance or proof of analytic visibility. Fine and Economy choose 0.25 and
1 pixel. View → Display detail exposes these choices and automatic refinement.

A separate persistent worker retains up to eight native configured caches.
Construction still uses the document's original modeling tolerance. Camera
changes remesh retained native shapes without rebuilding those shapes. Initial
cache misses and changed definitions can rebuild in this isolated worker while
navigation and authoritative evaluation continue independently. There is at
most one active refinement request. Source and generation checks reject stale
results. The last matching display detail remains available while a newer level
is pending. Work/deadline errors keep the original evaluation display and report
the reason; they never replace native geometry with guessed surfaces.

The disposable response is bounded to 256 targets, 500,000 triangles and
1,500,000 vertices. The existing process deadline bounds native subdivision
which cannot be interrupted within the adapter. Transport and mesh-budget
failures retain original display detail. Refinement preserves native face/edge
source identities, analytic curve metadata and native body inventory. History
stations have distinct output keys; a current-result mesh cannot replace an
earlier station. Assembly-owned deposited metal and edge-only reference curves
retain their original evaluated display caches.

`PresentationState.display_quality` stores view preferences independently from
CAD source. The `viewport-detail` semantic role exposes state, diagnostics,
commands and quality values. CSS-like viewport properties override preferences:

| Property | Values |
| --- | --- |
| `renderer-display-adaptive` | `true`, `false` |
| `renderer-display-chord-pixels` | 0.05–8 |
| `renderer-display-minimum-mm` | 0.000001–1 |
| `renderer-display-maximum-mm` | 0.000001–1 |

Reversed limits resolve to the minimum. Styling does not edit model settings.
User finish maps, physical tile sizes, relief depths and lighting values remain
unchanged. Native carrier normals and inverse-transpose occurrence transforms
retain smooth curved shading and sharp CAD edges. Imported grayscale height
pixels still drive grain through the existing footprint and slope filtering.

## Depth-aware sketch and datum outlines

Visible-depth overlays no longer classify entire 10-pixel fragments from a
single midpoint. They clip continuous projected line intervals against indexed
display triangles. Triangle coverage uses screen-space half-planes, and depth
comparison uses perspective-correct inverse depth (linear depth for
orthographic views). Near-plane clipping keeps intersecting display triangles.
Occluded intervals merge before visible segments are drawn.

A tiled index limits each line query to crossed screen cells, rather than
scanning every model triangle for each sample. It is cached by source identity,
camera, viewport/render aspect, history, edge/fabrication input-body selection,
placement/visibility state and display-detail epoch. Up to four viewport
contexts retain independent indexes. They share one approximately 1.5 ms frame
slice and aggregate limits of 500,000 stored triangles and 2,000,000 tile
references; those budgets are not multiplied by the number of user-defined
viewports. Index work continues on later frames, alternating pending active
contexts. Inactive contexts expire, and evicted contexts rebuild when used.
Each index also bounds source traversal to 500,000 triangles.
Optional visible-depth decoration is
omitted while the current index is incomplete or exhausted. Hover-cache tokens
expire initial misses when the index completes, including a stationary pointer.
Explicit X-ray overlays remain available and bypass depth clipping.

This visibility is precise for the current disposable triangle display, subject
to numerical depth bias and explicit resource bounds. It is not exact analytic
B-rep visibility and never constrains sketches or resolves source references.
Compilation, formatting and linting are implementation checks; no automated
tests, geometry probes or interactive acceptance runs accompany this change.
