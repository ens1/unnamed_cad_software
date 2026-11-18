# Drawing workspace

Drawing is a persistent paper workspace over the shared document, commands and
undo history. Entering it selects the previous surviving sheet, or the first
sheet; a document without drawings offers a blank paper workspace with New Sheet
and template actions. It does not create a draft merely by opening the workspace.
Model returns to the modeling workspace after the current drawing edit is
accepted or cancelled.

The default dock includes a sheet tree with views, dimensions, annotations and
production tables. Selecting a tree item selects the same paper target and opens
its properties. The same stable sheet, annotation and table identities drive
paper highlighting, direct handles, context actions and property fields. The
paper remains visible when Apply accepts an edit. Cancel restores the checkpoint
and keeps the surviving sheet visible. A target that disappears on cancellation
is cleared rather than redirected to another annotation.

Paper zoom, pan, active sheet/page and selection are presentation preferences.
They are retained during annotation edits and stored with the saved presentation
state, separately from engineering drawing source. Invalid or missing saved
sheet targets fall back to a surviving sheet. Old presentation files deserialize
with Drawing closed and default navigation.

New Sheet properties group model scope, paper format/orientation, projection and
shared scale. Standard Front/Top/Right views use third-angle projection. Users can
include Isometric, use Front only, or edit custom views. Scale presets and saved
custom scales use the existing f64 view layouts and validated draft. Existing
per-view placement and parent alignment remain editable. Choosing a preset that
removes an annotated view is rejected by drawing validation.

**Line display (all views)** sets Hidden lines removed, Hidden lines dashed, or
Wireframe on the sheet's ordinary projected views. Each view also has its own
Line display field under Views. These are source-owned settings, so Apply,
Cancel, undo, templates and exports retain the same choice. New sheets use
Hidden lines removed. Existing sheets without saved choices retain Wireframe
until edited. The tessellated Section Z contour keeps its separate behavior.

The default dock pins Apply, Cancel Edit, PDF/DXF and Model actions above the
scrolling properties. The sheet/view/annotation tree uses the dependency region
once, with compact properties below it. Export requires an applied, current validated drawing.
Each action and tree item is also an independent semantic binding. Existing
`drawing-editor` confirmation action identities remain available to custom
layouts; their default visual position has moved to the pinned footer.

## Welds on paper

Part and assembly sheets show symbols for the model’s deposited welds by default. Select a symbol and use **Show weld in view** to choose a projection. Front and Back show opposite sides of a joint, as do Left and Right or Top and Bottom. A symbol updates its leader and side when the view changes. Drag its handle to position it on paper, or use **Place automatically** to restore automatic view selection and placement.

Use **Edit weld in model…** to change the joint or mark it as a field weld. To hide generated symbols on one sheet, clear **Sheet → Model scope → Show model weld symbols**. Manual welding annotations have their own placement. The sheet preview, drawing PDF and drawing DXF share the resulting symbols. [Weld symbol conventions](drawing-symbols.md)

When you open a weld symbol, model dimension, or datum from the sheet, **Finish & return** brings you back to the same page, zoom, and pan. The updated callout briefly highlights. **Cancel** returns to the same spot and restores the original callout. If the edit removes the callout or moves it off the page, TCAD keeps your place and tells you.

## Model dimensions on paper

A new part sheet shows finished model dimensions and established datums automatically. **Sheet → Model scope → Show model dimensions and datums** also enables them on an existing sheet. Each callout follows its part and configuration. Select a label and choose **Edit in model…** to change its tolerance. Drag its handle to change where it prints. A label move is a sheet edit and uses Apply, Cancel and Undo.

TCAD projects a measurement once per sheet. A suitable orthographic view gets the measurement lines or angular arc. A foreshortened view uses a leader and the true model value. A projected diameter or radius carries its model size. Explicit part datums can be referenced from drawing tolerance frames. The Model callouts group and Drawing information show the generated annotations and any missing coverage.

PDF and drawing DXF use the same evaluated paper vectors. Exporting a part without a saved sheet generates a dimensioned projection. For an assembly without saved sheets, export also generates one local part detail for each represented solid part and configuration. If you have saved sheets, they determine the exported drawing set. Assembly sheets can show component dimensions once per represented part configuration. Datum letters stay scoped to their part detail sheets.

If a saved tolerance or datum loses its model reference, the part stays editable. Drawing export identifies the requirement to repair. A sheet whose automatic recognition reports omitted sizes carries a coverage note. When automatic callouts fill the sheet, the page selector adds model-dimension pages after any table pages. Each page repeats the views its callouts refer to. PDF and drawing DXF keep that order and sheet numbering across the drawing set. Dragging a callout pins it to its view and page. **Place automatically** returns a moved callout to automatic layout. Moving a pinned label outside the paper flags it for repair before export.

## Paper content and fidelity

Production paper, PDF and drawing DXF retain ordinary view labels, standard scale
labels, user-authored notes, dimensions and title-block metadata. Automatically
inserted evaluator descriptions, source hashes and internal document revision
footers no longer become paper vectors.

Those details remain available in Drawing Information and through the additive
`tcad.drawing-render-information` version 1 descriptors. Evaluated sheets publish
`source_info` (document, source revision/fingerprint and evaluator fingerprint)
and `view_fidelity` (per-view description, approximation and hidden-line status).
The rendering information is separate from the paper primitives. Explicitly
user-authored linked source fields in a title block remain honored.

Visibility now splits supported native rational edge curves into visible and
hidden pieces. Hidden pieces are omitted or drawn with short, even dashes;
ghost components and construction references keep their distinct chain strokes.
The desktop, PDF and DXF consume these same classified pieces. Native planar
faces with closed straight trims use their polygon domains. Other faces use
the evaluated surface tessellation, with sampled curved-face silhouettes.
Drawing Information reports the method and tolerances, including approximate
curved visibility. This is not a complete exact hidden-line solution for general
curved surfaces. Native dimensions keep their source references independently
of the display pieces. Section views remain tessellated. Invalid geometry,
reference attachment and paper layout still block production export; preview
repair diagnostics remain separate from production output.

## Presentation bindings

`drawing.workspace-open`, `drawing.workspace-model` and `drawing.tree-select`
address workspace/navigation intent. Tree roles are `drawing-tree-group` and
`drawing-tree-item`. Drawing information uses `drawing-source-info`,
`drawing-source-detail` and `drawing-view-info`. The pinned composition uses
`drawing-workspace-footer` and `drawing-workspace-action`. Model scope, paper,
projection and scale remain independently bound `drawing-editor-field` values.
Users may move, hide, duplicate or restyle these elements in their layout and
CSS-like theme without changing engineering source or Apply/Cancel semantics.

The native registry exposes `drawing-tree` and `drawing-completion` independently
from `drawings-editor`, so alternative layouts can move the tree and action bar
without duplicating their state.

Implementation verification is compilation, formatting and linting only; no code
tests or interactive acceptance were run.
