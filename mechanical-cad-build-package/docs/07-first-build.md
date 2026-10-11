# Historical first build and milestones

This document preserves the original foundation plan. The [root instructions](../../AGENTS.md), current product specification, and [mechanical CAD coverage inventory](../../docs/mechanical-cad-coverage.md) supersede its milestone-first, slice, probe, and manual-demonstration workflow. Build the complete application end to end; compile, format, and lint without writing/running code tests or manual smoke tests. Interactive acceptance belongs to the user. The exclusions and sequencing below do not reduce the approved minimum feature baseline.

The first build proves the core workflow with real geometry. Manufacturing drawings, full Git integration, and broad feature coverage remain in the product specification, but are not prerequisites for this slice.

Every milestone follows the [replaceability and presentation contracts](11-replaceability-and-presentation.md). Rust, Bevy, and Monstertruck are starting implementations; the rail is a default user-replaceable layout/theme. Do not write tests for any code. Milestone evidence comes from compilation/linting, runtime diagnostics, and manually exercised demonstrations and hardware sessions.

## Demo scenario

Load a small drive assembly containing a shaft and two instances of one support definition. Isolate one support, retain the shaft as a visible reference, and select a supported edge. Choose Fillet, adjust its radius in the instrument rail, and inspect the proposed result on both instances.

Cancel preserves the original definitions. Apply commits one coordinated edit. Undo restores it. Save, close the worker, remove caches, and rebuild from source. The workflow uses a trackpad and keyboard.

Use a simpler generated support during initial proofs. The visual render's complex bracket is not a claim that every illustrated operation already works.

## M0 — Prove dependencies and core capabilities

Build small, independent probes rather than the whole product.

- Resolve and pin Rust, Bevy, and Monstertruck; record exact features, licenses, compiler, lockfile, and platform.
- Create a real B-rep solid, perform extrusion and a hole/cut case, and tessellate it.
- Preserve triangle-to-face and edge-overlay provenance for picking.
- Find and validate a genuine single-edge fillet case; vary its radius across at least two successful values and one deliberately invalid value.
- Export/reimport STEP and compare expected closure, bounds, and geometry within explicit tolerances.
- Capture trackpad event traces through the actual Bevy/windowing stack.
- Probe editable numeric fields, focus, text selection, clipboard, units, and scrubbing without committing on gesture end.
- Show a semantic parameter/action binding in two different positions and styles using editable resources, without changing its data or command implementation.
- Document the initial subsystem contracts, implementation selection points, and replacement/migration boundaries.
- Establish a permissive solver evaluation plan for sketches and assembly placements.

**Exit:** reproducible commands, saved probe outputs, capability report, dependency pins, and named remaining blockers. A triangle-only approximation cannot pass the geometry probe.

If fillet or provenance support is insufficient, write the precise kernel/adapter task and a failing representative fixture. Improve that path before claiming the illustrated feature works.

## M1 — Document, transactions, and source

Implement stable identities, typed graph inputs/outputs, part definitions, instances, explicit placements, units, canonical save/load, and cycle detection.

Define theme-agnostic semantic UI projections and command bindings separately from document types, renderer objects, and layout/style resources. Record independent contract/schema versions.

Implement draft transactions and result acceptance separately from rendering. Add Apply/Cancel and undo/redo invariants, including stale/out-of-order result rejection.

**Exit:** a fixture round-trips with a clean diff; two instances reference one definition; Cancel leaves source unchanged; Apply is one undo entry; invalid dependencies fail clearly.

## M2 — Worker and viewport

Introduce a persistent worker, explicit adapter boundary, revisioned requests, diagnostics, B-rep validation, derived tessellation, and picking associations.

Render realistic materials with dependable navigation. Resolve selected faces/edges to source nodes and instance paths.

**Exit:** a real evaluated part can be inspected and picked while deliberately slow evaluation runs; worker restart invalidates handles and rebuilds from source.

## M3 — User-defined presentation and live edit

Implement the instrument rail as the bundled default layout/theme, open dependency stations, scoped graph/history modes, dedicated parameter instrument, exact entry, contextual operations, ghost comparison, circular Apply, and Cancel. Add user-defined composition and CSS-like styling through separate resources, including switching/reloading and recovery to the default.

**Exit:** select an edge, change a parameter, inspect the current validated ghost, Apply/Cancel, and undo. Move/regroup/restyle controls and switch between rail and a substantially different layout without recompiling. Both use the same draft/command bindings. Active drafts survive reload, invalid resources retain usable UI, and keyboard focus and trackpad routing remain correct.

## M4 — In-place assembly editing

Add active editing scope, isolation, Hide/Ghost others, pinned references, shared-instance indication, assembly peek, and Finish editing restoration.

**Exit:** a definition edit previews both support instances without opening a separate part document. Instance transforms remain independent.

## M5 — Authoring and basic constraints

Introduce the solver validated in M0 and implement constrained sketch creation/editing, supported solid features, and basic placement constraints.

Keep solver equation sets distinct from document dependency edges. Manually exercise underconstrained, overconstrained, inconsistent, and degenerate cases and record solver diagnostics.

**Exit:** the user can construct and constrain a simple part rather than only edit a loaded fixture, assemble it, and perform the earlier workflow.

## M6 — First usable modeling release

Expand representative geometry and exchange coverage, source recovery, diagnostics, and device/platform evidence. Add one promoted assembly parameter with coordinated geometry/placement preview.

**Exit:** the agreed constrained sketch/core solid/basic assembly scope is usable, complete workflows pass on supported hardware, and documented unsupported cases are explicit.

Do not estimate calendar completion from these milestone labels. Record measured progress and scope as the implementation proceeds.
