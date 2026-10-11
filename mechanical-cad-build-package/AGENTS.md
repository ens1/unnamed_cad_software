# Mechanical CAD implementation instructions

## Current instructions take precedence

Read the repository's [root AGENTS.md](../AGENTS.md) and [mechanical CAD coverage inventory](../docs/mechanical-cad-coverage.md). Build the complete application end to end; SolidWorks mechanical CAD is the minimum feature baseline, including modes, supported geometry, associativity, and manufacturing outputs. Missing kernel/solver capabilities remain in scope. Older M0-first, slice, probe, and demonstration workflows in this handoff are superseded. Do not write or run code tests, assertion probes, or manual smoke tests. Compile, format, and lint; leave interactive acceptance to the user.

## Read first

Read README.md, docs/00-decisions.md, docs/07-first-build.md, docs/11-replaceability-and-presentation.md, and implementation/FIRST-TASK.md. Use docs/01-product-spec.md for full scope and assets/ui/instrument-rail.png for the default layout reference.

This package belongs to a new mechanical CAD project. Do not apply unrelated repository dependencies, branding, or application assumptions.

## Approved direction

- Replaceable subsystems throughout; Rust core, Bevy desktop/viewport/input, and Monstertruck geometry are the initial implementations.
- Local-first files, no mandatory account or cloud, WTFPL original code.
- Trackpad and keyboard are primary.
- Select geometry before choosing an applicable operation.
- Real evaluated ghost preview, explicit Apply/Cancel, atomic undo.
- In-place assembly editing and shared-definition awareness.
- Theme-agnostic semantic UI information/actions, separate user-defined layout and visual styling.
- Empty Part1 and familiar CAD command tabs, feature tree/property manager, and one model viewport. Sketch-first modeling on planes or model faces; Flow is optional, with the instrument rail as its visual reference. Users can replace composition and appearance.
- Do not write tests for any code.

Changes to product behavior require explicit product discussion. Replacing an implementation while preserving product contracts is authorized by the replaceability requirement. Resolve ordinary implementation details with focused evidence and document them.

## Architecture rules

Keep document types free of Bevy entities, renderer types, and Monstertruck handles. Use stable application IDs and typed inputs/outputs. Graph, feature history, and viewport share one command path.

Put every subsystem behind an application-owned, versioned contract. Composition code selects implementations; consumers do not reach into adapters. Expose capabilities and optional extensions explicitly. A replacement must document semantic changes, migrations, reference effects, and unsupported cases.

Expose semantic UI data and commands independently of widgets, theme selectors, colors, and coordinates. Interpret separate user-editable layout and style resources. User layouts may move, regroup, hide, duplicate, and restyle any exposed element. Loading presentation resources never edits model definitions or bypasses Apply/Cancel.

Separate part definitions from instance placements and assembly structure from geometric dependencies. Never flatten graph branches just to fit the rail.

Keep f64 authority and explicit units/tolerances. Meshes are derived. Persist provenance-based references; never face indices or live kernel handles.

Use persistent worker processes for expensive geometry. Tag requests/results, reject stale results, invalidate crashed-worker handles, and keep navigation responsive.

Draft edits never mutate committed definitions. Gesture end never commits. Apply commits the current validated proposal once; Cancel invalidates its jobs and restores committed state.

If a root problem belongs to a kernel implementation, fix its adapter/kernel or replace the implementation through its contract. Keep upstream contributions licensed according to upstream requirements. Report unsupported geometry honestly.

## Implementation approach

Build the complete approved product. Use the coverage inventory to expose missing families and partial modes without treating the historical milestones as release boundaries. Implement real operations and their authoring/editing/persistence paths; do not add placeholder commands to inflate feature coverage.

Implement the default CAD workspace and optional instrument rail over the same semantic UI contract used by user layouts. Do not hard-code panel positions, ordering, colors, or shapes into domain or interaction logic.

Investigate the actual pinned API. Published features are not application verification. Record dependencies, licenses, tested platforms/devices, unsupported cases, and reproducible commands.

Do not introduce AI modeling or required services without explicit scope approval. Replacement kernels, renderers, toolkits, runtimes, and other implementations are allowed within the documented product and licensing contracts.

## Checks after a Rust workspace exists

~~~sh
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --bins --lib -- -D warnings
~~~

Use the actual crate names and applicable targets if they differ. Fix formatting, compilation, and lint issues. Do not write unit, integration, end-to-end, property, snapshot, or other code tests. Do not disguise tests as assertion scripts or automated probe suites.

Retain runtime validation and diagnostics. Record source-inspection/build evidence and implementation limits. Leave interactive transaction, geometry, persistence, and hardware acceptance to the user; do not run manual smoke tests or demonstration/probe commands as a substitute for code tests.

## Reporting

Lead with working behavior, then validation evidence and material limitations. Keep capability reports honest. Deliver focused changes and update decision records when a proposal becomes a tested choice.
