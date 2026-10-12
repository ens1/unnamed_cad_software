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

The default dock pins Apply, Cancel Edit, PDF/DXF and Model actions above the
scrolling properties. The sheet/view/annotation tree uses the dependency region
once, with compact properties below it. Export requires an applied, current validated drawing.
Each action and tree item is also an independent semantic binding. Existing
`drawing-editor` confirmation action identities remain available to custom
layouts; their default visual position has moved to the pinned footer.

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

This cleanup does not add hidden-line removal or exact sections. Supported native
edge carriers remain projected as exact vectors; unsupported carriers can be
sampled, and the current section view remains tessellated. Drawing Information
states those limits and whether hidden lines remain included. Invalid geometry,
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
