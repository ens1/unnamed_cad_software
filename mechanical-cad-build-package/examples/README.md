# Illustrative project fixture

[drive-unit/project.json](drive-unit/project.json) proposes readable multi-file source for a small assembly. It has two instances of one support definition and a shaft.

The source support is a simple plate with a through-hole, not the fully detailed bracket in the concept render. Purchased bearings, seals, fasteners, pulley, motor, and complex bracket features are omitted. Geometry, clearances, and fillet feasibility have not been evaluated. Use this fixture to settle domain relationships and persistence, then adapt it to the proven native schema and kernel cases.

## What it demonstrates

- Stable readable definition, instance, parameter, node, and reference IDs.
- A feature chain with explicit parameter branches.
- Two support instances sharing one definition but separate placements.
- A promoted assembly control referencing an existing part parameter.
- Presentation/isolation metadata separate from geometry.
- Theme-agnostic semantic bindings shared by a rail layout and a top-oriented workspace.
- Composition in [ui-layouts.json](drive-unit/ui-layouts.json), CSS-like visual rules in [ui-theme.json](drive-unit/ui-theme.json), and active-resource preferences in presentation.json.
- A draft changing the shared fillet radius from 3 mm to 6 mm.
- A semantic/provenance reference requiring unique resolution, rather than a face/edge index.

The source profile dimensions, coordinate axes, operation names, and schema version 0 are proposals. They are not an implemented file format. The outer_top_edge reference requires an actual construction/provenance resolver.

The UI resource grammar and binding paths are also proposals. Both layouts reuse the same component bindings and draft commands; styling contains no model values. Switching the active layout demonstrates the intended separation of information/actions, composition, and appearance. User-local preferences need not be stored in the project; these resources are bundled to illustrate explicitly shareable layouts.

During M1, implement schema validation and round trips. During M0/M2, generate and validate corresponding supported geometry. During M3/M4, use the draft fixture to verify previews, cancellation, shared instances, and undo.
Use manually invoked demonstrations and runtime validation; do not write code tests or automated assertion suites.

The draft's source_fingerprint placeholder must be computed from the actual committed source before evaluation. The fixture is not a request that can safely be sent directly to an implemented worker.

## Expected domain behavior

The committed radius remains 3 mm during preview and after Cancel. The draft shows 6 mm on both support instances when evaluated successfully. Apply changes the shared definition once and creates one undo record. No placement or geometry changes are written into presentation.json.
