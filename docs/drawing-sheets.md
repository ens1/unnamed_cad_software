# Drawing sheets, formats and templates

The Drawings property manager edits saved sheets through the shared document draft. Add, rename, move up/down, duplicate and delete sheets; choose the sheet shown in the main viewport; select its part/assembly scope and paper dimensions. A4/A3/A2 landscape, Letter and Ledger sizes are shortcuts to editable millimetre dimensions. Swapping orientation swaps the saved paper width and height.

Apply waits for the current validated evaluation and commits the entire drawing transaction as one undoable change. Cancel restores its starting checkpoint. Invalid local sheet fields remain editable while the previous valid geometry is available. An invalid local source cannot apply or export through the drawing controls.

## Identities and reference guards

Renaming or reordering sheets keeps their IDs. View keys (`front`, `top`, `right`, `isometric`, `section-z`, `detail-front`) are scoped to the sheet and remain unchanged by sheet/view reordering. Their semantic IDs combine the stable sheet ID with the view key. Removing a view that still owns dimensions or annotations fails until those items are moved or deleted; at least one view remains through the docked controls.

Duplicating a sheet creates fresh sheet, dimension, annotation, table, revision-row and title-cell IDs. It retains model feature/topology references, configuration references and occurrence paths. Balloons reconnect to the duplicated sheet's new BOM table while saved BOM numbering reservations stay intact. Duplication does not copy review comments.

Deleting a sheet or retiring a sheet-owned item rejects references from review comments. A BOM table cannot be removed while its balloons remain attached. Replacing a format creates fresh title-cell IDs and reports review references to retired cells. Sheet names and drawing-owned IDs must be distinct within the document.

## Borders, zones and linked title fields

The native sheet-format contract is `tcad.drawing-sheet-format` version 1. It defines a border, left/bottom/right/top margins, optional numbered column and lettered row zones, and an optional title block anchored to the bottom-right border interior. Zones support 1–100 columns and 1–26 rows, with finite millimetre spacing. There is no assumed ISO/ASME certification or projection-standard symbol.

A title block stores its width, height, text size and independent stable-ID cells. Each cell retains a label, relative X/Y/width/height rectangle and a typed value source. Fields can contain literal text or link to the document name/custom properties, model name/custom properties, active configuration name, part material, sheet name, sheet number/count, source revision or source fingerprint. Native configuration inheritance resolves part-property/material overrides; assembly custom properties currently come from configuration overrides. A material field requires a part sheet scope. Missing custom properties display blank values.

Cell borders and wrapped labels/values are shared f64 manufacturing vectors. Cell containment and overlap, finite coordinates and actual linked-text height are validated. Overflow reports the affected title field so it can be enlarged or its text size reduced. The current default export font/text support remains in effect; font selection, logos, arbitrary artwork and standards-specific title-block generators are not implemented.

Legacy sheets without a saved format retain their previous border and note/title-property presentation. Existing `title_block` text-map values remain supported alongside the new format.

## View layout and scale

Each supported named projection can retain an editable paper rectangle and an optional fixed model-to-paper scale. Enabling saved placement starts with the current evaluated rectangle; if evaluation is unavailable, it supplies an editable paper rectangle for repair. An omitted layout uses the existing automatic grid/fitting. An omitted scale fits to the saved rectangle. View order determines automatic layout without rewriting annotation bindings.

Rectangles must lie inside the border and leave room for view captions and reserved title/note/table space. A fixed standard-view scale that does not fit its native projected control hull is rejected. The existing detail projection clips to its paper rectangle. Parent-child projection alignment, arbitrary relative/auxiliary orientations and direct viewport dragging of view rectangles remain gaps.

Standard views retain supported exact projected rational curves. The existing `section-z` mode remains explicitly tessellated, and the detail mode uses display-curve clipping; these format/layout additions do not imply exact general sections or hidden-line removal.

## Local template files

Save Template writes a human-readable JSON sheet template with version 1, paper dimensions, format, view defaults/layouts, sheet-note defaults and legacy title metadata. Add Sheet from Template creates a new sheet for the selected model scope and generates fresh sheet/title-cell identities. Load Format from Template replaces an existing sheet's paper/format while preserving its model scope, views, annotations, notes, tables and their identities. Existing view rectangles must still fit the resulting paper.

Templates contain no model topology, component occurrences or live external-file link. The loaded format is copied into application-owned native source, so changing or removing the template file does not change saved drawings. Template input is limited to 1 MiB. Pagination, external CAD template formats and standards-library management remain gaps.

## Exploded assembly drawings

A sheet scoped to the current document-root assembly can select one of that assembly's saved exploded views compatible with the current root configuration. It consumes the final evaluated exploded stage. Native occurrence paths and definition/configuration keys remain attached, so associative annotation placements move with the same instances. Connector lines pass through each drawing view's actual f64 projection and become shared dashed vectors.

Missing or incompatible exploded caches fail explicitly. Nested/non-root assembly exploded caches, intermediate exploded stages, per-sheet independent configuration selection and arbitrary alternate-position views are not supported. Mechanical placements, quantities, inspection, STEP and STL remain unchanged.

The main-viewport sheet preview, drawing PDF and drawing DXF consume the same sheet border/title/view/annotation vectors. Compilation and lint are engineering checks; interactive drawing acceptance remains with the user.
