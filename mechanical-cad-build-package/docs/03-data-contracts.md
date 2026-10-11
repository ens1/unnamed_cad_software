# Document and data contracts

These are implementation proposals for the first slice. The examples are illustrative source fixtures, not a completed native format or evaluated CAD output. Finalize the schema through manually exercised M1 save/rebuild demonstrations.

## Identities

Allocate stable application identities for documents, part definitions, assembly definitions, instances, nodes, parameters, and references. Never derive an identity from array position, display name, geometry similarity, or a kernel address.

The fixture uses readable IDs for inspection. Production ID allocation must preserve uniqueness within its documented scope and survive rename, reordering, save/load, and ordinary edits.

An evaluated instance is identified by its assembly instance path plus part definition ID and revision. Two instances can share one definition while retaining separate placements and presentation state.

## Proposed entities

| Entity | Essential fields |
|---|---|
| Project | Schema version, project ID, root assembly path, definition file map |
| Part | Definition ID, named parameter map, typed node map, designated body outputs |
| Assembly | Definition ID, instances, explicit placements, placement constraints, parameter consumers |
| Node | Stable ID, operation kind/version, named typed inputs, outputs, reference definitions |
| Parameter | Stable ID, unit/dimension type, literal or expression, bounds/constraints where applicable |
| Reference | Owner definition, producer node/output, topology kind, semantic role, resolution policy |
| Semantic UI | Stable semantic IDs/roles, values/draft text, units, relationships, states, diagnostics, command availability and bindings |
| Layout resource | Versioned component/container composition, semantic bindings, order, sizing, positioning, visibility, responsive rules |
| Style resource | Versioned selectors, cascade/overrides, variables, colors, typography, spacing, borders, shapes, interaction appearance |
| Presentation | Active layout/theme resources, component view preferences, graph positions, camera, active scope, isolation and visibility pins |
| Evaluation | Source fingerprint, process generation, opaque shape handles, validity, provenance, meshes |

Place presentation metadata in a separate file. Persist durable design definitions, not a dump of Bevy ECS state.

## Source layout

~~~text
project.json
parts/
  bearing-support.json
  shaft.json
assemblies/
  drive-unit.json
presentation.json
ui-layouts.json         # illustrative shareable layouts; user preferences may live outside the project
ui-theme.json           # styling separate from composition and semantic data
cache/                 # disposable; excluded from Git
imports/               # external assets, with fingerprints
~~~

Use explicit units in source and typed, canonical units internally. A literal length of 8 mm converts once to the core's canonical representation. Expressions carry dimension information; reject adding an angle to a length.

Distinguish references to parameter definitions from references to their evaluated values. Promoting a part parameter to an assembly control references the same identity and expression definition.

## Typed graph

Nodes declare named input/output ports and dimension/geometry types. Body-producing features identify their primary body input and designated output for history presentation. A feature sequence is not the complete dependency graph.

The example bearing support uses Sketch → Extrude → Bearing bore → Fillet → Body. Thickness and fillet radius are independent parameter definitions referenced by the appropriate features. Body is an explicit display/output designation; it need not correspond to another kernel operation.

Shared sketches, parameters, and reusable groups stay shared. Detect dependencies before evaluation. Invalid reordering is rejected atomically. Layout changes never change evaluation order.

## Topological references

A reference names the producer/output and an intended topology role. An example role is outer_top_edge, but a role name alone is not a resolver.

Implement role definitions from construction provenance and operation history. Record how an operation maps source topology to new, split, merged, and removed topology. A reference policy can require exactly one surviving candidate or explicitly permit a particular split set.

A geometric signature may help identify candidates, with recorded tolerances, but it must not silently choose among ambiguous faces. Return resolved, unresolved, or ambiguous diagnostics. Persist the user's explicit resolution choice when appropriate.

No reference reconstruction or capability demonstration may depend on a face's enumeration index remaining unchanged.

## Definition versus placement

The assembly fixture instantiates one support definition twice. A definition-level radius edit affects both instances. Editing an instance transform affects only placement. If an instance requires different geometry, explicitly create a configuration or separate definition.

The example fixtures contain explicit placement values and no general assembly solver implementation. Later placement constraints belong to their own solver node or module and remain separate from feature geometry.

## Serialization and versions

Canonicalize object ordering, key order, numeric formatting, and units deliberately. Preserve useful names and stable IDs. Reject non-finite geometry values and unknown unsupported schema versions. Validate references before overwriting source.

Record schema, operation implementation, kernel, and solver versions. Source formats migrate explicitly. Cache invalidation does not migrate source. A kernel upgrade requires geometric/reference validation, not just changing a dependency version.

Use temporary writes and a recoverable project transaction for coordinated changes across files. Store imported geometry and texture assets separately from readable definitions.

Version subsystem and semantic UI contracts independently of layout/style resource schemas. Preserve semantic identities across visual reloads and adapter changes. A document/persistence replacement migrates definitions explicitly; a kernel/solver replacement invalidates incompatible derived data and reports reference changes.

Layout/style resources refer to semantic roles and bindings, not live widgets or kernel handles. They never contain authoritative parameter values or command implementations. Presentation preferences belong to user-local state by default; explicitly shared resources use portable paths. The fixtures demonstrate a rail and a top-oriented workspace over the same bindings.
