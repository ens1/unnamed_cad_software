# Saved assembly display states

Display states save component visibility and visual finish arrangements. They
remain independent of the camera, engineering material, mechanical placement,
mates, suppression, mass properties and CAD exchange geometry.

Use the Assembly manager's **Display States** controls or command search to
create a named state. Add the selected component or choose a component path,
then set its visibility and finish. **Visible**, **Hidden** and **Ghost** are
separate from **Inherit**. Finishes come from the project finish library. The
editor uses the shared draft and undo history; the viewport updates as source
fields change. Apply waits for the current evaluation and texture readiness.
Cancel restores the previous source checkpoint and active display selection.

The viewport picker selects a saved state or **Normal Display**. Switching this
picker changes presentation state only: it dispatches no document command or
geometry request, moves no camera and creates no model undo entry. Editing a
saved definition is an undoable document operation. Deletion remains pending
until Apply; Cancel restores it. Creating or copying a state opens its editor.

## Scope and inheritance

Each `Assembly.display_states` map owns named definitions. A definition can
apply across all configurations or be linked to one configuration. Activating
a linked state requires that configuration to already be selected; it never
switches configuration or silently edits component suppression. An inactive
configuration scope shows normal appearance and an explicit diagnostic.

Targets are complete occurrence paths relative to the owning assembly. A
subassembly target affects its descendants; the most specific path independently
wins for visibility and finish. Full owner occurrence paths distinguish repeated
subassemblies. Default controls author root assembly states; semantic actions
also support explicit `assembly`, `state` and `owner_path` arguments for nested
assembly occurrences. An explicitly selected occurrence can establish a nested
state's owner scope.

Hidden components contribute no viewport geometry, fit bounds, picking or hover
center guides. Ghost components show a translucent finish and are excluded from
viewport picking and hover guides; they remain addressable from the tree and
saved state editor. Temporary isolation still applies after saved visibility.
Component finish overrides in the state affect the displayed part finish;
explicit native face finishes and optical weld bead finishes take precedence.
Manufacturing/BOM finish requirements continue to use model/configuration
assignments and never infer requirements from an active display state.

Missing or suppressed targets retain source records and publish diagnostics;
they do not attach to another component by position, definition name or leaf ID.
An inactive target has no visual effect. Removing a component therefore need not
discard a saved arrangement; a user can explicitly repair or remove its path.
Finish deletion is blocked while any saved state references that finish.

## Contract, limits and presentation

The versioned application-owned `tcad.display-states` contract contains no Bevy
entities, materials, meshes or camera transforms. `Definition` carries a stable
ID, name, optional configuration and ordered source override records. Each
override has its own stable ID, full path and optional visibility/finish.
`PresentationState.display_state` carries the selected assembly, state and owner
path. Old documents omit empty state maps and absent selections, preserving
their serialization. Component library linking namespaces state, override,
occurrence, configuration and finish identities.

Limits are 128 states per assembly, 4096 overrides per state, 65536 source
overrides per document and 64 levels per occurrence path. The source scope cache
supports one million configured occurrence prefixes; the desktop picker lists
up to 4096 paths after bounded traversal. Selected components can establish a
path directly. The disposable effect cache holds up to 65536 occurrence paths.

Cache identities include the authoritative source fingerprint, document/session
identity, draft edit, active configurations, owner scope and selected state.
Source or scope changes reject prior effects. Optical caches are disposable and
never establish design intent. Editable controls resolve override IDs against
the current editor; index pointers are display hints. Per-editor tokens reject
stale control bindings after reopening. Invalid advanced JSON remains visible
and repairable without mutating the last valid source definition.

Semantic roles include `assembly-display-state`, `display-state-component`,
`display-state-picker`, `display-state-field`, `display-state-action`,
`display-state-source`, `display-state-description` and
`display-state-diagnostic`. `workspace.display-state` exposes the active picker
with a `display-state-selection` setter. Definitions, exact scope, choices,
inheritance, actions and diagnostics remain available to custom layouts. The
default picker and editor fields use the common semantic control renderer and
CSS-like styling.

A drawing can explicitly save `Drawing.display_state` in its sheet model scope.
The sheet's Projection controls choose a saved state or Normal display. Advanced
source can supply a nested assembly owner path, relative to `Drawing.definition`.
A viewport selection never silently changes a drawing. Changing sheet model scope
clears the old explicit display/exploded selections. Sheet duplication retains
its selection; reusable templates omit these model-bound identities.

Hidden components and their explode connectors are excluded from projected
geometry and fit bounds. Ghost components use dashed reference strokes in the
paper workspace, native PDF and DXF. Exact projected rational curves remain
Spline primitives and DXF SPLINEs; their reference line type uses a 4/1/1/1 mm
pattern. Desktop and explicit DXF polyline strokes share bounded continuous dash
geometry. Native PDF uses the same physical dash pattern. Production vectors
remain black line art; optical finish assignments are preserved in source and
render metadata, without pretending to produce textured drawing illustrations.
This adds no geometric hidden-line removal or ray tracing.

Mechanical `sheet_instances` remains unfiltered for associative dimensions,
annotations, BOM, hole tables, cut lists and inspection. Existing annotations
therefore stay associated and are preserved; hidden/ghost model geometry is
excluded from new paper reference picking. A linked state requires a compatible
active assembly configuration. Incompatible or missing explicit states reject
production rendering, rather than exporting an unrelated fallback. Retained
absent target records report diagnostics and affect no projected component.
The `drawing-display-state-information` role publishes the source scope,
hidden/ghost/finish counts, diagnostics and vector fidelity. A state in use by
a drawing cannot be deleted until that drawing's selection changes.

Native CAD exports remain mechanical source exports, unaffected by viewport
visibility; only an explicit drawing selection changes that drawing's vectors. No tests or UI
acceptance runs were performed; compilation, formatting and linting are the
implementation checks.

The DXF reference line type follows the group-code structure documented by [Autodesk LTYPE](https://help.autodesk.com/cloudhelp/2020/ENU/AutoCAD-DXF/files/GUID-F57A316C-94A2-416C-8280-191E34B182AC.htm).
