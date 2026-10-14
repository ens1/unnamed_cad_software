# Conveyor sketch interaction corrections

This implementation addresses the sketch portions of P2-01 and P2-05 through
P2-08 in the conveyor review. It changes desktop authoring and presentation over
the existing application-owned sketch, constraint, gesture and draft contracts.
It does not replace the native sketch solver or make display polylines
authoritative.

## Persistent inference

With Snap Entities enabled, a 2D drawing click near the projected sketch origin
uses that origin and saves a driving `coincident` relation to `@origin` for the
authored point or center. Reusing an existing point at the origin also adds the
missing origin relation, without duplicating a relation already present. The
visible inference badge distinguishes this persistent relation from ordinary
grid placement. Alt bypasses both entity inference and grid snapping, leaving
the independently authored coordinate seed unconstrained.

Multi-click tools retain the snap intent of each click. Center and endpoint
tools preserve origin inference in their authored controls. Circle perimeter
references use retained points with Point on Circle relations. A snapped polygon
center uses a construction-circle center and vertex-on-circle relations; snapped
slot centers use Arc Center and, for a centered slot, a construction centerline
with a Midpoint relation. These are real sketch definitions and relations,
excluded from solid material contours when they are construction geometry.
They do not fabricate model topology. Conic authoring now honors the saved
per-click join intent rather than silently merging a coincident seed when Alt
requested independent placement.

3D origin inference retains its persistent fixed part-origin point. An existing
unconstrained point at zero no longer masks origin inference. Its viewport badge
and semantic metadata now explain which persistent reference will be saved.
Origin inference is distinct from the current 3D drawing workplane and the
world-coordinate seed used for a numeric point.

## Move versus driving coordinates

The 2D point inspector names its numeric controls **Move X** and **Move Y** and
states that they move a solver seed under existing dimensions and relations.
Changing those values records a sketch undo checkpoint. Their displayed units
and precision follow model settings; unchanged display rounding never writes
back to authoritative f64 coordinates.

**Constrain X** and **Constrain Y** open the corresponding driving origin
coordinate in Smart Dimension, with the existing live preview, Done Dimension,
Cancel Dimension and Finish Sketch behavior. Existing coordinate dimensions
remain editable through the same dimension path. Origin, X-axis and Y-axis
buttons add persistent reference relations. Operation-generated points retain
their source-driven restrictions rather than accepting contradictory coordinate
edits. Only the selected point exposes primary coordinate inputs; a separate
collapsed, virtualized point browser remains available for the complete sketch.
Source restoration reconciles the selection by stable point/member identity,
so coordinate editing does not jump to a different point after source ordering
changes.

3D coordinate fields already save driving Coordinate relations. They now say
**Driving X/Y/Z from Origin** and publish that intent explicitly, distinguishing
them from handle movement under existing constraints. Expression fields use the
shared quantity presentation: model precision for inactive display, exact
authored source when editing, and no source mutation merely from focus.

## Picking and framing

2D picking now ranks candidates in screen points, independent of world scale.
Close point/center hits have deliberate priority; outside that small center
region, the nearer member can win over an endpoint's larger drag target. Native
point handles participate in the same decision instead of unconditionally
stealing a nearby member click.

All, Points, Members and Construction filters affect native 2D and 3D geometry
selection. Filters change presentation only. Nearby 2D candidates have a compact
chooser and a highlighted current candidate; Tab cycles them while the viewport
owns input. Keyboard focus and key consumption keep this separate from field
traversal. Candidate entities, labels, selection actions, filter actions and
inference information are semantic descriptors. Chooser placement and appearance
use the existing overlay layout and style resources.

Fit Sketch Selection frames selected 2D members, points or dimension references;
the existing Fit Sketch, Fit Support and Fit Editing Part commands remain.
Selected 3D geometry has its own Fit Sketch Selection action. These use derived
display bounds for camera framing and do not alter source geometry. 3D Tab still
cycles workplanes, consumes the gesture and leaves text-field traversal alone.

## Stable dimension input and numerical presentation

The nearby Smart Dimension input captures its viewport anchor when opened and
retains it while a new value moves the solved geometry. Theme layout overrides
remain authoritative. The input stays available if its geometry annotation moves
outside the viewport. Closing or canceling that dimension discards the temporary
anchor. Docked and nearby fields continue to bind the same expression and draft.

Cursor coordinates and the point browser now use model-selected length units and
precision. Existing f64 seeds and full generated expressions remain in source;
the shared expression editor and quantity semantic descriptors supply compact
display text. Parameters and authored equations remain intact.

## Scope and evidence

This work does not add new constraint equation families or solve unrelated
kernel geometry limits. The nearby chooser is for active 2D sketch entities;
the global model/assembly candidate chooser is integrated separately. 3D uses
the shared selection filters and closest-geometry ranking while retaining Tab
for workplanes. Reference-direction and radius/size clicks that do not represent
an authored sketch point remain governed by their existing tool definitions;
origin inference is persisted through actual retained point/center references.

Formatting, `cargo check -p cad-desktop`, strict
`cargo clippy -p cad-desktop -- -D warnings`, and scoped `git diff --check` were
performed during integration. No tests, assertion probes, geometry runtime
probes or interactive acceptance checks were written or run. The rebuilt
application still requires the user's acceptance of these interaction changes.
