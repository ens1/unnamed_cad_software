# Revision clouds

Drawing Annotations includes **Revision cloud** with rectangular, elliptical and
closed-polygon outlines. These authoring modes correspond to the revision-cloud
shapes documented by [SolidWorks](https://help.solidworks.com/2025/english/SolidWorks/sldworks/c_Revision_Cloud_Overview.htm?id=13.7.19);
its [rectangular cloud workflow](https://help.solidworks.com/2024/english/SolidWorks/Sldworks/t_Inserting_Rectangular_Revision_Cloud_HELP.htm)
also describes direct corner resizing. TCAD's cloud is a saved paper annotation,
with an explicit attachment, outline, scallop parameters and optional revision
association. It does not infer model changes.

Add a cloud in the selected drawing view, then select its outline on paper. The
ordinary attachment handle can reattach it to a supported current native model
or sketch point. Cloud sizes and vertices are paper millimeters; their native
attachment follows model changes while paper size remains explicit. Move the
cloud with its center handle. Rectangle and ellipse corner handles resize in
the cloud's rotated local frame and keep the opposite corner fixed. Polygon
handles move independently identified vertices. Invalid or stale gestures use
the existing drawing capture and restoration rules.

The docked editor exposes width, height, paper rotation, maximum scallop chord
and bulge. Polygon editing selects one vertex at a time, with X/Y fields,
midpoint insertion, removal and an explicit reset to box corners. Polygon
vertices and rectangle/ellipse dimensions remain independent when switching
outline mode. A polygon retains 3–128 stable vertices and must have a nonzero
area, distinct sides and no crossing, touching or backtracking boundary. Shorter
scallops improve narrow or concave regions; automatic collision cleanup is
separate work.

Scallops are positive-weight rational quadratic circular arcs, bounded at 2048
arcs per cloud. Rectangle/polygon sides divide into chords no longer than the
saved pitch. Elliptical seeds use a conservative angular chord bound. Preview,
PDF and drawing DXF use the common f64 paper-vector pipeline rather than a
renderer-only overlay or model tessellation.

Choose an existing revision-table row to link the triangular callout. The saved
table and row IDs retain the association across revision text changes. A prefix
is optional; an unlinked cloud may carry its own authored label. Show/hide,
paper text size and independent label offsets are editable, and the callout has
its own drag handle. Adding a cloud selects the last row of the first revision
table when available. Revision rows/tables referenced by clouds cannot be
deleted until those clouds are reattached, unlinked or removed. Sheet duplication
refreshes table, row, annotation and vertex IDs and remaps the links to the copied
sheet.

Cloud fields, actions, source identity and direct handles publish semantic data
for user layouts and CSS-like styles. Drawing contract 10, revision-cloud contract
1 and paper-interaction contract 5 preserve source ownership independently of
widgets and kernel objects. The cloud shares the existing drawing draft,
current-validation Apply, Cancel and one-operation undo.

Freehand clouds, stand-alone sheet-space clouds, automatic model-difference
regions, linked release state, leader-bound callouts and automatic scallop
intersection cleanup remain gaps. Compilation, formatting and strict linting
are the implementation checks; no tests or UI smoke runs are performed.
