# Decisions and status

## Approved product choices

| Area | Decision |
|---|---|
| Ownership | Free, open-source, local-first; ordinary files; no required account or cloud |
| Platforms | Windows, macOS, Linux |
| Original code | WTFPL |
| Architecture | Ship of Theseus: every subsystem independently replaceable through application-owned contracts |
| Core | Initially Rust document, evaluation, persistence, transactions, CLI |
| Application | Initially Bevy scene, viewport, input, desktop shell |
| Geometry | Initially Monstertruck analytic/NURBS B-rep integration |
| Modeling semantics | Application-owned typed dependency graph |
| Mechanical CAD baseline | Mainstream capabilities shared by SolidWorks, NX, Onshape, and Creo, including practical modes, associativity, solver capability and manufacturing outputs; the user's latest 2026-10-10 clarification replaces full SolidWorks parity |
| Baseline boundaries | Simulation, routing, CAM, electrical design, and PDM product-family parity are not part of this minimum; retain TCAD's local-file and Git requirements |
| Views | Graph, familiar feature history, and viewport share authoritative definitions |
| Primary input | Trackpad and keyboard |
| Interaction | Select geometry, discover operations, adjust, preview, Apply/Cancel |
| Assembly editing | Isolate and edit in place; retain context and selected references |
| Shared definitions | Preview all affected instances; explicit alternatives for differing geometry |
| UI semantics | Theme-agnostic information and commands, independent of visual presentation |
| UI composition | User-defined layout and styling; familiar CAD command bar, left feature tree/property manager, and one model viewport by default; instrument rail guides the optional Flow presentation |
| Rendering | Default realistic metal, rubber, and plastic; user-defined appearance with explicit status semantics |
| Version control | Human-readable source and meaningful Git workflows |
| Engineering checks | Do not write or run code tests, probes, or manual smoke tests; compile, format, and lint; retain runtime validation and leave interactive acceptance to the user |

Rust, Bevy, Monstertruck, and all other implementation choices can be replaced at any development stage through their contracts. Preserve the product requirements, document migrations and capability differences, and keep unsupported cases explicit. A replacement kernel is not assumed to produce identical topology. See [replaceability and presentation](11-replaceability-and-presentation.md).

## Current delivery scope

Build the complete application end to end. The mechanical CAD baseline is a minimum, with breadth and depth tracked in the [coverage inventory](../../docs/mechanical-cad-coverage.md). Existing narrow geometry support is an implementation gap, not a reduction in product scope. Older milestone-first, slice, probe, and manual-demonstration instructions in this package are historical and superseded by the repository's root instructions and subsequent user feedback.

## Default presentation

The current default is an empty Part1, Features and Sketch command tabs, a left feature tree/property manager, named Front/Top/Right planes and Origin, and one model viewport. Sketching happens on the actual part or an origin plane in that viewport. The [instrument rail](../assets/ui/instrument-rail.png) guides the optional Flow presentation: continuous sections, dependency stations, and a dedicated measurement area. Users can replace the layout, positioning, grouping, colors, typography, and control appearance without changing CAD semantics or compiling the application.

The earlier Flow renders preserve approved behaviors: selection-first actions, pending previews, assembly isolation, and coordinated parameter edits. Their card shapes and chrome differ from the default rail; user layouts may use either form or an entirely different composition.

## Implementation proposals

The following are starting proposals, to validate rather than present as completed decisions:

- Bevy native UI with application-owned visuals and headless widget behavior.
- A small custom station/route graph view over the domain graph.
- Application-owned semantic UI descriptors, with separate declarative layout and CSS-like styling resources.
- Rust 1.99.0, Bevy 0.20.0, Monstertruck 0.4.1 as researched starting candidates.
- Versioned JSON fixture files for the earliest slice; evolve a documented source schema deliberately.
- A persistent worker process and a simple framed IPC protocol for the first slice.
- Explicit assembly placements before general constraint authoring.
- Start with a few cohesive crates; split modules when ownership and compile-time needs justify it.

## Open choices

Constraint solvers, final editable-field toolkit, layout/style grammar, native input adapters, reference-resolution strategy details, and production asset sources require evidence. See [open decisions](10-open-decisions.md).

A development capability report must distinguish published library capabilities, cases verified by this application, and unsupported or untested cases.
