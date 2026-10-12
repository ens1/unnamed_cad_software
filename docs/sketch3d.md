# 3D sketches

Choose **Sketch → 3D Sketch** or **Insert → 3D Sketch** to create spatial
geometry in the model viewport. Double-click its feature-tree row to edit it.
The property manager, viewport and customizable semantic controls all edit the
same pending feature. **Finish 3D Sketch** waits for the current validated native
evaluation and commits one undoable operation; Cancel restores the checkpoint.

## Drawing and editing

The editor supports points, connected lines, three-point arcs, oriented circles,
Bézier control curves, clamped positive-weight NURBS and interpolating splines.
Every member can become construction geometry. Points retain absolute f64 XYZ
coordinates in the part's frame, including when editing a placed occurrence.

Drawing uses XY, YZ or ZX input planes. Tab changes the input plane through the
last point. Plane depth and explicit XYZ fields provide precise placement;
snapping uses the configurable grid, point identities and optional axis
inference. Alt bypasses those placement inferences. Input planes are transient
editor state. The saved reference frame is an explicit source setting used by
downstream structural-section clocking; moving an input plane does not alter it.

Select points or members, using Command/Ctrl to retain multiple selections.
Spatial point handles move in the current plane or along XYZ axes. The persistent
solver worker evaluates drag goals with the existing dimensions and relations.
Release retains the solved edit pending confirmation; Escape restores that
gesture's checkpoint. Stale source, editor and sequence results are discarded.
No temporary drag goal is saved in the sketch definition.

Smart Dimension supports XYZ coordinates from the part origin, signed axis
distances, point-to-point distance, straight-member length, circular radius,
line-to-line or line-to-axis angles, and shortest distance to a supporting line.
Driving expressions use the shared unit/parameter evaluator; reference
dimensions measure the current solution without adding equations. The editor
also exposes fixed XYZ, coincidence, world-axis alignment, parallel,
perpendicular, equal length, point-on-line/curve, workplane and tangent relations.
Solver diagnostics report residual and local degrees of freedom.

## Modeling use and limits

Spatial members can supply Composite Curve and datum point/axis sources. Solid
Sweep and Curve Driven Pattern accept a selected native member or a connected
nonbranching material path. Surface Sweep accepts a connected open polyline with
its existing fixed-orientation restriction. Structural Member accepts selected
spatial straight members and a catalog section with locating point, roll and
offsets. Existing native carrier/frame/junction admission limits still apply.

Spatial sketches contain wires and controls, with no body count, mass or material
region. Closed spatial contours do not automatically become extrudable planar
sections. Surface-attached curves, model-edge/face conversion into associative
spatial sketch members, general curvature constraints and curved spatial stock
remain separate work. Circle orientation is authored frame intent rather than
an independent rotational solve variable. General spline arc-length driving is
not supported. Editing an occurrence with a different forced configuration
requires selecting the matching part configuration; configured free-contact
seeds can limit dragging with an explicit diagnostic.

## Replaceable contracts and styling

The application-owned `sketch3d::Definition` v1 is embedded in
`Operation::Sketch3d`; existing planar sketches and schema-1 files remain intact.
Native kernel objects do not enter saved source. Evaluated spatial geometry,
motion/residual diagnostics and drag requests use f64 DTOs through versioned
adapters and persistent workers.

The `sketch3d-*` roles expose groups, fields, notes, actions and entity selection
independently of layout. The `sketch3d` value adapter, `sketch3d.action` dispatch
and `sketch3d-editor` view adapter are available to CSS-like themes and custom
workspace compositions. See [presentation resources](presentation.md) and
[native geometry details](sketch3d-native-geometry.md).

Completed spatial curves and standalone points respect the Sketch display
Auto/All/Hidden mode and source selection. Their individual overlay roles expose
color, stroke/handle size, visibility and depth styling, including construction
and selected states. Editing uses one curve overlay rather than overlapping a
second native wire display.

Compilation, formatting, linting and bundle signature verification establish
integration and packaging. Interactive acceptance remains with the user; no code
tests, geometry probes or UI smoke runs are part of this implementation.
