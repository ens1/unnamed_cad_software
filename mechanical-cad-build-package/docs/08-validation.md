# Runtime validation and user acceptance guidance

Do not write or run code tests, assertion probes, or manual smoke tests. Compile, format, and lint implementations; retain runtime schema/dependency/geometry validation. Leave interactive acceptance and physical usability evaluation to the user. The scenarios below preserve product acceptance guidance; they do not authorize the coding agent to run demonstrations, probes, or manual tests. The [mechanical CAD baseline](../../docs/mechanical-cad-coverage.md) supersedes the original small-corpus scope.

## Domain correctness

Manually inspect stable identity after rename/reorder/save/load; typed port validation; shared parameters; explicit instance-versus-definition edits; cycle rejection; body-history projections of branching graphs; and deterministic source serialization.

Validate schema migration and atomic multi-file recovery before exposing it to real projects. Fixtures must preserve topology policies without face-index assumptions.

## Geometry corpus

Begin with extruded plates, one through-hole, stepped shafts, a supported fillet, and repeated support instances. Add brackets, housings, tangent/coincident/near-degenerate Boolean cases, chained fillets, shells, offsets, sweeps, and lofts as scope expands.

Check closure, validity, expected bounds/volume where available, and tolerances. Use independently derived expected properties for some cases; comparing the kernel to itself is insufficient evidence.

A STEP round trip checks geometry and placement within tolerances, not string equality. Fresh-process reconstruction checks references after deleting caches.

## Transactions and concurrency

Cover stale results, edits arriving out of order, Cancel during evaluation, Apply during evaluation, base revision changes, repeated Apply, worker death/restart, failed preview, shared-instance edits, and undo/redo with pending jobs.

Document definitions remain unchanged by preview and Cancel. One Apply commits every linked change as one undo operation.

## Presentation and usability

Inspect the bundled instrument rail's continuous sections, open stations, visible branches, dedicated parameter instrument, circular Apply, exact units, keyboard focus, text scaling, and access to actions with many fields. User layouts may replace this form entirely.

Manually edit composition/style resources, reposition/regroup/restyle controls, and switch between the rail and another layout. Confirm semantic data and command bindings remain the same, duplicated fields share a draft, reload preserves editing context, malformed resources yield useful diagnostics, and the reset mechanism recovers hidden controls.

Selection resolves the same geometry identity in rail and viewport. Contextual operations reflect the selected topology type. Failed or pending evaluations cannot present a valid-looking old result as current.

Verify physical material rendering, edge contrast, ghost distinction, and legible annotations at practical display sizes.

## Input and platforms

Use built-in MacBook and external Apple trackpads, Windows precision touchpads, and representative Linux touchpads in supported windowing environments.

Record event traces and actual operation results. Manually exercise navigation and scrubbing during evaluation, scrolling in user layouts, small geometry, occlusion cycling, modifier transitions, finger repositioning, focus loss, cancellation, momentum suppression, and available gesture fallbacks.

Measure responsiveness, frame pacing, selection mistakes, accidental edits, and fatigue. Initial engineering targets may include a 60 Hz camera/selection path and measurements on a normal laptop; they become release promises only after measured budgets and supported hardware are agreed.

## Per-milestone reporting

Include reproducible commands, versions, platform/device details, observed results, representative artifacts, and explicit limitations. Distinguish build/lint evidence, runtime diagnostics, and physical usability observations. Demonstrating one supported fillet case does not establish arbitrary fillet robustness.

## Replacement evidence

Document each subsystem's contract and selection point. When replacing an implementation, manually exercise affected workflows, record capability differences and required migration/rebuild, and invalidate incompatible caches. Keep semantic UI and source identities independent of the implementation being replaced. Do not add speculative implementations solely to demonstrate abstraction.
