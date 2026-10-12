# Drawing annotation cohorts and cleanup

Paper selection supports a bounded cohort of 128 items. Command-click toggles an
item; Shift-click extends the selection. Every selected item highlights, and the
most recently selected item supplies the active property fields and handles.
Right-clicking an already selected item retains the cohort and makes that item
the layout reference. A normal click selects one item, background clears the
selection, and **Clear paper selection** is also a semantic command.

Annotation cleanup appears for annotation cohorts. Notes, datums, datum targets,
feature-control frames, surface/weld symbols, balloons, dimension groups and
revision clouds can be arranged or translated. Center marks and centerlines
retain their fixed native attachments and support cohort deletion. Dimensions,
views, tables and title fields remain selectable but use their existing specific
layout controls; mixed item types disable annotation cleanup rather than
silently moving only part of a selection.

The cleanup operations are:

- **Align origins in a column/row** sets the corresponding paper coordinate of
  each annotation placement origin to the active reference annotation.
- **Even horizontal/vertical origins** needs three annotations and spaces their
  placement origins between the two current outer origins. The endpoints remain
  fixed.
- **Row/Column at spacing** uses the active reference first, followed by cohort
  selection order. A row extends rightward and a column downward.
- **Duplicate annotations** creates new source annotations at the saved copy
  spacing in X and Y, then selects the copies.
- **Delete annotations** removes the exact selected source identities.

These operations align native-resolved f64 placement origins. They do not
certify text-font outlines, equal visual gaps, or automatic collision avoidance.
The native attachments stay fixed. The resulting paper offsets are saved in
ordinary drawing definitions and drive the same preview, PDF and DXF vectors;
the commands do not create persistent alignment constraints that reapply after
later model changes.

Drag a selected movable annotation or its Move handle to translate the entire
movable cohort. Every member uses the same paper delta from the gesture's source
checkpoint. Native references and existing leader-elbow offsets remain unchanged,
matching single-annotation Move: leaders reroute between the fixed attachment,
their separately authored elbows, and the moved label. Elbows and independent
cloud/group label handles remain individually editable. Selected members are
excluded from peer-bound snapping during a group drag. This extends existing
paper/grid snapping; it is not a general geometric snap solver.

Duplication refreshes annotation, dimension-group target, feature-control-frame
row and revision-cloud vertex identities while preserving native model references
and revision-table links. Copied ordinary datums receive unused letters; copied
targets receive unused letter/number labels. Geometric-tolerance references among
copied datums and frames remap to the new letters. Deleting a datum required by
a retained frame is rejected until the frame is reattached or included in the
deletion. Existing shared drawing reference-retirement guards still apply.

Commands, selection membership, active reference, semantic item toggles and
copy/stack spacing are exposed independently of the native widgets. Paper item
styles can target the `primary` and `cohort` classes. Human-readable item labels
replace raw IDs in the normal semantic browser; identities remain in structured
source data. Layout contract 1 and paper-interaction contract 5 reject stale
selection/source data, use the existing document draft and current validation,
and retain Apply/Cancel/one-operation undo.

Cross-sheet clipboard transfer, geometry-relative spacing, text-font edge
alignment, persistent placement constraints and general mixed-item transforms
remain gaps. Compilation, formatting and strict linting are the implementation
checks; no tests or UI smoke runs are performed.
