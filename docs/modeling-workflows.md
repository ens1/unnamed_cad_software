# Everyday modeling controls — 2026-10-11

This application pass improves selection, sketch feedback, explicit reference
repair and presentation. It does not add new Boolean, blend or freeform carrier
algorithms. Geometry acceptance still depends on the supported kernel families
in [edge treatments](edge-treatments.md) and the
[coverage inventory](mechanical-cad-coverage.md).

## Native selection sets

An edge selection offers **Select complete rim / loop** and **Select tangent
chain**. A face offers **Select face boundaries**, including its actual outer
and inner wires. The Fillet and Chamfer property manager exposes the same set
actions and lets each listed edge be focused, highlighted or removed. Expanding
a set inside either editor updates its authored edge list through the common
draft; preselection outside an editor remains view state until used by a feature.

`cad-core::selection_sets` contract v1 uses optional native incidence in
`EdgeData`. Vertex, component and wire tokens identify only the current mesh
cache; they are never saved as construction references. Native endpoint
positions and supported exact endpoint derivatives establish incidence and
tangency. Display polylines do not establish either. Unique derivative
continuation can follow a complete cylinder/cone rim through cap-sector vertices
with incident spokes; unambiguous degree-two incidence can follow other closed
loops. Branching or incomplete/unknown tangency returns an explicit reason.
General intersection-carrier tangent inference is not implemented.

Expansion stays within one body, native component and full occurrence path.
It requires the current evaluation or the exact feature checkpoint/input-history
evaluation used by picking. Older display fallbacks cannot author an expanded
set. Traversal, topology and source-byte limits bound work. Very large bodies
may omit optional incidence metadata while retaining their geometry and display;
set expansion then requires a supported native cache rather than guessing.

The semantic commands are `selection.set-boundary`, `selection.set-loop` and
`selection.set-tangent`. Existing stable-source edge add, clear and remove
commands are joined by `selection.edge-focus/...`. Availability and failure
reasons are projected independently of widgets. Expanding a complete set does
not imply that an arbitrary selected blend network is geometrically supported.

## Sketch dimensions and relations

The sketch property manager groups dimensions and relations with **Selection**,
**All** and **Unsatisfied** filters. Selection relates members to their actual
endpoints/centers and constraints to their referenced entities; it does not
spread through neighboring geometry into the whole sketch. Selecting a relation
emphasizes its participating members, points, origin or axes without replacing
the actual sketch selection. An Unsatisfied filter emphasizes geometry involved
in currently unsatisfied equations. Dimension/relation annotation visibility
remains saved presentation state.

Dimension values and driving/reference modes use the existing sketch history,
solver and shared draft. Contact constraints also expose the existing supported
fixed/free parameters and opposed tangent intent. A local edit refreshes the
geometry and equation feedback before Apply. Failed solves keep the last valid
body preview and block confirmation.

The feedback cache retains an exact successful `SolveReport` only for its current
source. Failed solves display unknown degrees of freedom. Per-constraint equation
vectors come from the current authored geometry using the same curve-dimension,
derived-geometry and residual-block pipeline as the solver. A component above
`1e-6` is unsatisfied; angle equations retain the solver's 100 mm/radian scale.
Nonfinite or uncomputable feedback has an explicit error. These vectors are not
a minimal conflicting constraint set and are not the unavailable final candidate
of a failed solve.

`<sketch-id>::feedback` publishes the report, residuals, filter and error state
and binds to the `sketch-feedback` panel renderer. `sketch-feedback-filter`,
`sketch-annotation-toggle`, `sketch-constraint`, `sketch-constraint-delete` and
`sketch-dimension-mode` controls use semantic IDs and theme rules. Typed fields
use the `sketch-feedback` setter with a current sketch and constraint identity.
Duplicate custom controls edit the same source and transaction.

## Sketch freedom and repair

**Freedom** in the sketch relation panel displays one or two local movement
arrows, or a constrained marker, on selected points and the points of selected
members/relations. With no selection it displays every analyzed point. Point
handles also distinguish locally constrained positions while retaining existing
constraint-aware drag behavior. Derived points display the movement inherited
from their exact source constructions; the overlay does not make them directly
editable. Arrow lengths, line widths, colors and visibility use the
`sketch-point-motion` style role.

The new **Dependent** filter exposes exact authored duplicates and equations
whose positional derivatives add no independent directions in authored relation
order. Zero positional derivatives, including direct curve-size expressions,
are identified separately. Local dependence can be intentional and is not proof
of conflicting design intent. **Remove duplicates** removes only exact equivalent
source equations, preserving grouped-dimension IDs. Command/Shift-click relation
rows to select several dimensions; **Selected → Reference** makes their values
measurements while retaining their driving expressions. Both actions preview as
one sketch history edit and remain inside Apply Sketch / Cancel Sketch.

Graphical motion/dependency analysis runs in a separate persistent worker after
a 90 ms debounce. Exact source, sketch/editor token, document generation,
revision/draft edit and backend guards reject stale replies. New in-flight work
is cancelled when edited or closed; idle workers persist across edits. Pending,
unavailable and retry controls are presentation state. Immediate equation
residual and exact-duplicate feedback remains available while motion updates.

The application-owned diagnostics contract v1 uses the same variable layout,
curve-size derivation, saved construction pipeline and residual blocks as native
solving. A numerical nullspace maps to f64 point derivative frames. Analysis is
limited to 512 free coordinates, 2048 equations and two million point derivative
entries. Exceeding these limits only removes graphical motion feedback, with an
explicit reason. Current source identity and valid-solve guards prevent stale
arrows after failed edits. This is local infinitesimal positional feedback;
finite travel, singularity certification and minimal nonlinear conflict sets
remain unimplemented. Solver fingerprint advances to `tcad-native-lm/13`.
See [diagnostics contract](sketch-diagnostics.md).

## Explicit reference repair

**Tools → Repair References…** opens a docked manager. It can continue an
already failed feature draft while preserving that operation's original
checkpoint. Choose a feature and input, choose a usable cached upstream stage
when needed, and explicitly select a compatible replacement. Topology references
can be picked on the upstream model; feature, sketch and member/point references
also have typed choices. Sketch support planes can be replaced by named origin
planes. **Preview repair** authors the replacement and reevaluates the shared
draft. Further repairs can be accumulated. Apply commits one undoable operation
only after the current evaluation passes; Cancel restores the starting checkpoint.

`cad-core::reference_repair` contract v1 inventories exact typed source slots,
including bound body and sketch-element companions. Replacement checks the
current expected record, source type, suppressed state, feature order and
document dependencies. It never searches for a nearby geometric substitute.
Final evaluation must establish geometric compatibility; an input-type check
alone does not certify a replacement face, support or carrier.

Feature-construction failures now return a current-identity invalid Evaluation
with targeted diagnostics, blocked dependent features and actual usable upstream
meshes/history. Independent construction failures are collected; downstream
consumers of failed inputs are skipped. This display is never marked valid and
never substitutes a previous document's shapes. Picking refuses pending or stale
results. Document validation, resource, configuration and assembly failures that
occur outside feature construction still return explicit worker errors; they
do not receive invented feature targets or partial geometry. Repair is not
general import healing or automatic topology rebinding.

`reference-repair-manager` binds to the `reference-repair` renderer. Failure,
input, upstream-stage and replacement choices publish typed arguments; native
controls use `repair-*` roles. Ordinary layouts can relocate or duplicate the
manager and its controls. Repair and modeling use the existing persistent worker
and stale-result rejection.

## Compact presentation

Default selected tree/history rows keep the same compact height as other rows;
operation details appear in hover text instead of expanding the selected row.
The viewport toolbar has separate ready, selected, editing and inspection sizing
states. Its default height is 28 px when idle and 56 px with secondary controls.
These dimensions are theme rules, not document constraints.

`viewport.toolbar`, `viewport.start-hint`, `viewport.view-cube`,
`viewport.candidates`, `viewport.isolation` and `viewport.navigation-hint` now
bind to independent registered renderers. Their default placement remains in the
single model viewport. Custom layouts can place them in ordinary regions and hide
their original overlays with CSS. Context, selection, candidate, navigation and
validation text are also semantic elements. The view cube resolves against its
actual layout bounds when docked. An Actions binding cannot replace an active
modeling or sketch editor.

The backend fingerprint advances to adapter 25 for the native selection metadata
and current-source failure display. The Monstertruck fork pin and solver version
are unchanged. Compilation and static checks do not establish interactive
acceptance; no code tests or manual smoke tests are part of this implementation.
