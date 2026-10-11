# Start here

Build the complete TCAD application end to end. The approved minimum is the **SolidWorks mechanical CAD feature set**, including modes, supported geometry, associativity, and manufacturing outputs. The user clarified this baseline on 2026-10-10. Historical milestone-first, slice, spike, and probe workflows are superseded.

Read these current sources:

1. [Repository instructions](../AGENTS.md).
2. [Mechanical CAD coverage inventory](../docs/mechanical-cad-coverage.md): required capability families, partial implementations, and missing modes.
3. [Product specification](docs/01-product-spec.md).
4. [Replaceability and presentation](docs/11-replaceability-and-presentation.md).
5. [Implementation task](implementation/FIRST-TASK.md).
6. [Current implementation architecture](../docs/architecture.md), [native format](../docs/native-format.md), and [presentation](../docs/presentation.md).

The original [first build](docs/07-first-build.md) and [foundation backlog](implementation/BACKLOG.md) are historical context. They do not defer surfaces, advanced assemblies, drawings, sheet metal, weldments, molds, configurations, or exchange beyond the approved complete-product scope.

Keep these requirements visible:

- Replaceable application-owned contracts; Rust, Bevy, and Monstertruck are initial implementations.
- Authoritative f64 definitions, explicit units/tolerances, stable IDs, exact B-rep operations, and recoverable local files.
- Semantic UI data/actions separate from user-defined layout and CSS-like styling.
- Empty Part1, familiar CAD command tabs and feature tree/property manager, one model viewport, and sketch-first modeling on the actual part or an origin plane.
- Optional Flow guided by the instrument rail; demo models are explicitly opt-in.
- Context actions on actual geometry, draggable constrained sketch handles, Smart Dimension, and live previews.
- Shared document/draft/commands/undo; current validated Apply, checkpoint-restoring Cancel, and cached History inspection.
- Persistent geometry workers, stale-result rejection, responsive navigation, and complete camera rotations.
- Mechanical feature breadth and depth beyond labels; kernel/solver restrictions remain gaps to close.
- Retained dependency licenses and distinct bundle filenames: TCAD, tcad-worker, tcad-cli.

Do not write or run code tests, assertion probes, or manual smoke tests. Compile, format, and lint; retain runtime validation and leave interactive acceptance to the user. Report actual supported modes and limitations without claiming complete parity from a build or feature list.
