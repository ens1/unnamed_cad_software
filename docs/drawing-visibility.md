# Drawing line visibility

Drawing contract 12 and sheet-template contract 4 save two independent choices
per ordinary view:

- Line display: Wireframe, Hidden lines removed, or Hidden lines dashed.
- Visibility geometry: Native surfaces, or Display mesh (approximate).

The editor provides sheet-wide and per-view controls. Missing line-display
entries retain the wireframe interpretation of old documents. New sheets use
hidden lines removed. Missing geometry entries use native surfaces. Templates
read versions 1–4; duplication, undo, local files and Git preserve both choices.
An unsupported native result never switches itself to sampled visibility.
Its error offers Wireframe or an explicit Display mesh choice.

View placement, scale, dimensions and annotations remain independent. Layout
interaction uses the cheaper layout calculation. Full drawing preview,
validation and exports use the saved visibility choices. Section-Z retains its
separate sampled section behavior.

## Native geometry and authority

Version 1 application-owned drawing geometry travels with evaluated faces and
edges. It stores original homogeneous rational controls and knots, analytic
sphere/torus/revolution carriers, native UV trims and occurrence placements.
No renderer or Monstertruck objects enter the core drawing contract. Display
triangles do not decide native visibility. This data is disposable evaluated
geometry; authored definitions and stable construction references stay separate.
Dimensions measure their original model/sketch references, never a drawing
stroke or visibility subdivision.

The kernel adapter obtains native face trims from the exact trim provider.
Missing carriers or trim definitions are explicit metadata errors. It retains
UV coordinates and orientations without converting angular parameters into a
NURBS parameter frame. Rational closure uses exact arithmetic on stored binary64
values. Supported full-period bands receive a derived closed UV chart with
virtual seams, while retaining their original loops. Virtual seams do not
become drawing edges. The physical difference between the stored angular period
and the real trigonometric period has an outward bound within the model budget.
General unproved periodic charts and unsupported trim joins remain errors.

## Native visibility calculation

Outward interval arithmetic evaluates native carriers, first/second derivatives
and trim curves. Trigonometric enclosures include argument reduction and Taylor
remainder bounds. Rational knot intervals retain the original controls and
cover every intersected span. Native trim parity includes outer and hole loops.

For a complete curve interval, native surface enclosures exclude occluders by
projected position or depth. A covering inverse-map inclusion proves an actual
trimmed surface in front of every point before declaring that interval hidden.
Subdivision resolves changes within the modeling tolerance. Local source-face
exclusion requires a proved injective projected patch; it does not exempt an
entire curved face or body from self-occlusion. Unproved classification and
exhausted work return errors, not a successful incomplete drawing.

An entire original edge whose complete projected enclosure is no larger than
one eighth of the modeling tolerance is retained as `collapsed_projection`
metadata and produces no paper stroke. This rule is checked over all original
knot spans before subdivision; repeated tiny cells cannot erase a longer edge.

Curved silhouettes retain the implicit equation
`(surface_u × surface_v) · view_direction = 0` on the native carrier. Regular
root cells, derivative bounds, trim membership and visibility determine their
finite output. A simple-fold proof handles local tangent rays while other
patches remain possible occluders. Admitted end-on ruled revolution faces have
a separate exact algebraic proof that their projected image is already covered
by native boundary curves. This avoids treating ordinary cylindrical end views
as an arbitrary singular surface.

`Sheet.native_traces` retains the native definition, bounded parameter cells,
occurrence placement, view, classification and model-space output error beside
the finite paper strokes. Visibility uses the model tolerance. Finite curve
output has a separate certified chord bound from the display/export accuracy,
in model millimetres; that bound does not decide occlusion. This avoids forcing
thousands of paper segments for each circle while retaining the native source.
The model tolerance is not the looser Boolean search tolerance. Native classification does not mean symbolic infinite-
precision PDF/DXF geometry or that every possible surface is admitted.

## Explicit sampled option and output

Display mesh visibility retains the existing numerical planar-polygon path and
sampled occlusion/silhouettes for other faces. This path remains available for
interactive use when native admission cannot establish the requested drawing.
Its fidelity identifies the sampled chord tolerance. It does not claim a
certified visibility result. Ghost geometry provides reference strokes but does
not occlude; hidden components are excluded in either mode.

Each view reports `wireframe`, `native_surfaces`, `native_planar`, `tessellated`,
or `section_intersection` fidelity. Native surfaces report both the modeling
tolerance and the finite curve-output bound. Sampled geometry, unsupported wireframe carriers, detail clipping and
repair clipping are marked approximate. The older native-planar numerical path
is distinct from the outward-interval native-surface path.

Paper, PDF and DXF consume the same classified primitives. Native traces use
bounded finite polylines; their exact implicit/parametric definitions stay in
the evaluated sheet metadata, not embedded in production PDF/DXF. The existing
rational wireframe and sampled-visibility edge paths retain supported rational
SPLINE/LINE output. Hidden strokes use the separate `TCAD_HIDDEN` line style;
ghost/reference strokes keep their long-short pattern. General exact sections,
hatching, auxiliary views and symbolic export of implicit silhouettes are
separate work.

## Limits and validation

Degree, control, integer-bit, subdivision, output and aggregate-work budgets
bound the calculation. Singular freeform silhouettes, unresolved tangencies,
unproved trim closure/charts, unsupported carriers and exhausted bounds remain
explicit failures. This implementation does not establish universal exact
hidden-line removal for arbitrary imported or freeform B-reps.

Evidence for this change is source review, compilation, formatting and linting.
No code tests, geometry probes, manual smoke runs or interactive acceptance
runs are part of this work. The saved release-review models and external
PDF/DXF consumers still need user acceptance.
