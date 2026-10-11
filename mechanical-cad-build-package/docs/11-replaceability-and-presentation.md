# Replaceability and user-defined presentation

## Ship of Theseus architecture

Build the application so any implementation can be ripped out and replaced at any development stage. This applies to the document engine, evaluator, command/undo system, geometry kernel, solvers, worker transport, persistence, renderer, desktop shell, input adapters, widgets, layout engine, styling engine, CLI, and version-control integration. Rust, Bevy, and Monstertruck are starting implementations.

Each subsystem has an application-owned contract describing its inputs, outputs, identities, units, capabilities, lifecycle, errors, and version compatibility. Consumers depend on that contract. Vendor objects stay inside adapters, and composition code selects implementations. Define boundaries around actual responsibilities; add modules/crates as needed rather than creating an abstraction for every function.

Keep dependency direction explicit and avoid global services or cross-module access that bypasses a boundary. A renderer replacement must not require redefining parameters; a widget replacement must not require changing transactions; a solver replacement must not require rewriting the viewport.

Expose optional capabilities and versioned implementation extensions so replacement does not reduce every subsystem to the smallest shared feature set. Persist extension identities and versions where they affect design intent. Report unsupported capabilities explicitly.

Contracts can evolve through documented versions and migrations. Replacing persistence or the document engine must preserve or migrate source identities and definitions. Replacing a kernel or solver can change geometry, topology, or solutions; record its fingerprint, invalidate incompatible caches, reconstruct references, and surface unresolved or ambiguous results. Replacement is supported without promising numerically identical output from different implementations.

A replacement record identifies the changed implementation, affected contracts, capabilities, migration/recovery path, required rebuild or restart, and observed limitations. The architecture must permit replacement throughout development; live hot swapping is an optional lifecycle capability rather than an assumed property of every library.

## Semantic UI contract

Expose CAD information in a theme-agnostic presentation model with stable semantic identities, roles, and relationships. It includes document/context identity, assembly instances, graph nodes and ports, history, selections, parameter values and draft text, units, expressions, bounds, diagnostics, preview status, shared-instance scope, viewport scene descriptors, and available commands with disabled reasons.

Expose every application-owned UI surface through this contract, including menus, dialogs, tooltips, viewport annotations, handles, and overlays. New functionality publishes its semantic information/actions before adding a default visual representation; no feature gets a permanently privileged hard-coded presentation.

This model contains no Bevy entities, widget handles, pixel coordinates, palette values, or assumptions about a left rail. Widgets and renderers consume projections of it. All views of a parameter bind to the same identity and draft; all actions invoke the same domain command path. View-only interactions use presentation commands separately from modeling commands.

Command identity and behavior are independent of its icon, label placement, visible shape, or whether it appears in a panel, menu, keyboard binding, or another user-defined surface. Accessibility labels, focus intent, units, and status meaning remain available independently of visual styling.

## Layout and style resources

Users define layout and appearance in separate, human-readable, versioned resources. The goal is CSS-like control over composition and presentation; the implementation does not require an HTML/browser runtime.

- Layout defines containers, bindings, ordering, grouping, docking, sizing, alignment, relative or absolute positioning, visibility, and responsive rules. Users may move, hide, duplicate, or regroup any exposed element, including the viewport, graph, parameters, context, and actions.
- Styling addresses semantic roles, stable element IDs, classes, and states through selectors, precedence/cascade, and reusable variables. It controls colors, fonts, spacing, borders, shapes, viewport appearance, and selection/preview appearance. Renderer-specific appearance capabilities are explicit extensions. Theme values must not be embedded in semantic descriptors.
- Interaction semantics remain separate from layout and styling. The actual laid-out element supplies hit geometry; gestures target its semantic identity. Positioning or recoloring a control does not change its command or permit invalid Apply.

Ship the instrument rail as an ordinary bundled layout/theme using this same interface. Give user resources the same composition and styling surface as bundled resources. A different layout may use a top toolbar, a right panel, floating groups, multiple views, or another arrangement. The rail's width, station form, circular Apply, and palette are defaults rather than restrictions.

Support local editing, switching, import/export, inheritance/overrides, and reloading layout/style resources without rebuilding the application. Keep existing drafts, selection, camera, isolation, and graph identities when presentation reloads. End obsolete gesture captures and reconcile focus by semantic identity. Loading a theme never changes authoritative engineering materials, dimensions, or model definitions.

Parse and validate presentation resources at runtime, report useful diagnostics, and retain the last usable presentation on failure. Provide a recoverable bundled layout and a discoverable reset shortcut or launch option so users can recover after hiding controls. Save user preferences separately from shared CAD source; project presentation resources can be shared explicitly and use portable relative paths.

The [layout](../examples/drive-unit/ui-layouts.json) and [theme](../examples/drive-unit/ui-theme.json) fixtures illustrate two compositions over the same semantic information. Their grammar and bindings are proposals to settle during implementation.

## Engineering policy

Do not write tests for any code. This includes unit, integration, end-to-end, property, snapshot, and other automated code tests, including assertion scripts or probe suites used as substitutes.

Do not run code tests, assertion probes, or manual smoke tests. Keep compilation, formatting, linting, runtime input/schema/geometry validation, and user-facing CLI validation/export features. Leave interactive acceptance and physical usability evaluation to the user. Record implementation limits and the evidence actually available without treating compilation as proof of CAD capability.

The [shared mainstream mechanical CAD baseline](../../docs/mechanical-cad-coverage.md) applies across replaceable implementations. The user clarified that full SolidWorks parity is not required; the target is equivalent practical capabilities shared by SolidWorks, NX, Onshape, and Creo. Capability declarations describe current support and gaps; they do not reduce required workflows to the limits of the selected kernel, solver, or widgets.
