# Architecture

## Ownership boundaries

The domain document owns design intent. The presentation model exposes semantic information and commands. Layout/style resources own visual composition and appearance. The kernel adapter owns evaluated geometry. Files own persisted design definitions. Rust, Bevy, and Monstertruck are the initial implementations behind replaceable contracts. No viewport mesh or UI field becomes the source of truth.

~~~mermaid
flowchart LR
    D --> S[Semantic UI information and actions]
    S --> R[Replaceable widgets and view adapters]
    L[User layout and style resources] --> R
    R --> C[Domain commands]
    V[Replaceable viewport selections and handles] --> C
    F[Native source files] <--> D[Authoritative document]
    C --> D
    D --> Q[Committed evaluation or draft snapshot]
    Q --> W[Persistent geometry worker]
    W --> K[Monstertruck adapter]
    K --> B[Validated B-rep and provenance]
    B --> T[Tessellation and picking metadata]
    T --> V
    B --> X[STEP export]
    D --> H[Headless CLI]
~~~

The input layer routes gestures into semantic camera, graph, selection, and edit actions before these reach a renderer, widget implementation, or domain commands. Targets are semantic identities with hit geometry supplied by the current layout, not hard-coded panel coordinates.

Every module is replaceable through its application-owned contract. Composition code selects implementations; consumers cannot bypass the adapter. Contracts describe version compatibility, capabilities, lifecycle, errors, identity, and units. Replacement may require an explicit migration and rebuild, especially for document, persistence, kernel, and solver changes. See [replaceability and presentation](11-replaceability-and-presentation.md).

## Start with clear modules

| Module | Owns | Must not own |
|---|---|---|
| document | Definitions, identities, typed inputs/outputs, instances, units | Bevy entities or kernel handles |
| commands | Validation, atomic edits, undo/redo, draft lifecycle | Widget state |
| evaluation | Dependency order, invalidation, draft requests, result acceptance | Camera/UI |
| kernel adapter | Monstertruck calls, tolerances, B-rep validation, topology history | Parametric document semantics |
| worker transport | Request lifecycle, process generation, diagnostics, cancellation | Editing decisions |
| input | Gesture normalization, routing, semantic actions | Authoritative dimensions |
| viewport | Render meshes, materials, picking, highlights, cameras | Saved topology identities |
| semantic UI | Presentation projections, identities/roles, values, states, command bindings | Widgets, layout coordinates, theme values, vendor types |
| layout/style | User composition, selectors/cascade, visual rules, reload and recovery | Authoritative geometry or modeling semantics |
| view adapters | Widgets, graph/history views, default rail and user compositions | A separate editable model or fixed product-wide layout |
| persistence | Canonical source, schema migration, atomic saves, cache metadata | Visual widget serialization |
| CLI | Shared core evaluation, validation, export | Alternate modeling engine |

[REPO-LAYOUT.md](../implementation/REPO-LAYOUT.md) proposes how to place these modules in a workspace.

## Document graphs

Maintain three distinct structures:

1. Physical assembly structure: definitions and instance placements.
2. Typed dependency graph: features, parameters, references, and derived placement inputs.
3. Familiar body history: a projection using designated primary body inputs/outputs.

Constraint equation sets can contain mutual mathematical relationships; solve them inside solver nodes. Acyclic evaluation dependencies between solver outputs and downstream features remain explicit. Reject a true dependency cycle rather than confusing it with an internal constraint system.

The default rail can display a mostly vertical route while retaining branches, shared definitions, and multiple outputs. User layouts can present that information elsewhere or with different visuals. A simplified presentation must not erase dependencies in the semantic contract. Expand or focus the graph when needed.

## Geometry boundary

Use f64 for domain geometry, units, tolerances, and placements. Convert to render-space values at the scene boundary; use a documented origin strategy for large coordinates.

Kernel calls return opaque process-local shape handles, validity diagnostics, operation provenance, and derived meshes through an application-owned adapter contract. Explicit capability discovery and optional versioned extensions preserve implementation-specific features. References persisted in files point to semantic/provenance definitions, never triangle indices, face enumeration order, or live kernel addresses.

A tessellation carries a mapping from triangle ranges to the corresponding evaluated face identity. Edge overlays carry equivalent edge associations. Picking resolves the hit to an evaluated reference and then to its source feature and instance path.

## Workers and caches

Use a persistent native process for expensive geometry operations. The UI stays responsive while the worker evaluates. Serialize owned request data; do not pass Rust pointers or assume every kernel object is transferable between threads/processes.

Start with a bounded length-prefixed serialized envelope over local process pipes, separating diagnostics from the transport stream. Validate payload size and protocol version. Optimize bulk tessellation transfers later if profiling justifies it.

Tag every request with base document revision, draft identity, draft edit sequence, worker generation, and the tested kernel/solver fingerprint. Cancel superseded jobs, but reject their results even when physical cancellation is unavailable. A worker crash invalidates its handles and triggers a rebuild from source.

Caches are disposable. A cache key includes relevant definitions, tolerances, external resource fingerprints, operation implementation versions, and kernel/solver versions. An independently rebuilt model must reconstruct references or report why it cannot.

## Commands and persistence

All model changes go through transactional domain commands. Validate their dependencies and reference policies before committing. Apply is one transaction across affected features, placements, and shared instances. Undo/redo restore definitions and trigger evaluation as required.

Store source by part, assembly, and reusable graph definition. Save through an atomic file strategy with recovery metadata for coordinated multi-file updates. Keep camera position, isolation, graph arrangement, and rail width in separate presentation state.

Provide the same evaluation and validation path through the desktop app and CLI. CLI correctness is independent of Bevy. Desktop ECS state mirrors the document and current evaluation; it does not replace them.

## Rendering

Initially use Bevy for the viewport, behind a replaceable render/scene adapter. Keep renderer-owned objects outside document, semantic UI, and layout/style contracts. Default to a broadly usable physically based raster path. High-end rendering experiments are optional; clear edge selection, frame pacing, and ordinary laptop usability are release priorities.

Render committed, proposed, and neighboring ghost geometry as semantically distinct layers. The default theme retains material appearance on the committed model and uses localized outlines and hatching. User themes can change visual treatments while the layer identities and status information remain available. Rendering overrides never change authoritative engineering materials.

## Presentation composition

Publish theme-agnostic data and command descriptors before instantiating widgets. Separate resources define component bindings and layout from selector-based visual styling. The default instrument rail and user layouts use the same contract. Repeated controls share parameter/draft identities, and loading presentation resources cannot execute modeling commands.

Reload or switch presentation resources without recompilation, preserving drafts and semantic selection. Reconcile focus and gesture capture as elements move or disappear. Runtime diagnostics retain the last usable layout on failure; an independent reset path restores the bundled default.
