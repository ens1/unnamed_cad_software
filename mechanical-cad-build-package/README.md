# Mechanical CAD build package

**Current scope:** build the complete application end to end with the SolidWorks mechanical CAD feature set as the minimum. Read the repository's [root instructions](../AGENTS.md) and [coverage inventory](../docs/mechanical-cad-coverage.md). The historical M0-first, slice, and probe instructions below and in supporting documents are superseded. Compile, format, and lint code; do not write or run code tests or manual smoke tests. The user performs interactive acceptance.

This is the implementation handoff for the local-first mechanical CAD application developed in this design discussion. The product name remains open.

The initial stack is Rust + Bevy + Monstertruck. Build like a ship of Theseus: every subsystem is replaceable through application-owned contracts. UI information and commands are theme-agnostic; users define composition and appearance through separate layout and CSS-like style resources. The default workspace uses familiar CAD command tabs, a feature tree/property manager, and one model viewport. The **instrument rail** guides optional Flow: a continuous left panel, open graph stations, a dedicated parameter instrument, circular Apply control, and realistic materials. Trackpad interaction is a core product requirement.

Do not write or run code tests, assertion probes, or manual smoke tests. Retain compile/format/lint checks and runtime validation; leave interactive acceptance and physical usability evaluation to the user.

![Instrument rail reference for optional Flow](assets/ui/instrument-rail.png)

## Start building

1. Read [START-HERE.md](START-HERE.md).
2. Review [the coverage inventory](../docs/mechanical-cad-coverage.md), [architecture](docs/02-architecture.md), [replaceability and presentation](docs/11-replaceability-and-presentation.md), and [the Flow rail reference](docs/05-instrument-rail.md).
3. Put this package in the new CAD project's repository and give your coding agent [FIRST-TASK.md](implementation/FIRST-TASK.md).
4. Build the complete application using replaceable contracts, shared draft/command/undo state, persistent geometry workers, and the familiar CAD default workspace.
5. Keep the full product specification and mechanical CAD coverage inventory aligned with actual implementation. Missing operations and partial modes remain required work.

## What is included

- [Full product specification](docs/01-product-spec.md), including manufacturing and Git workflows.
- [Mechanical CAD coverage inventory](../docs/mechanical-cad-coverage.md), including the approved SolidWorks baseline and current gaps.
- [Decisions](docs/00-decisions.md) separating approved choices from implementation proposals.
- Architecture, replaceability contracts, semantic UI and user-defined presentation, document contracts, preview/undo rules, trackpad behavior, and manual verification criteria.
- A prioritized backlog, repository layout, dependency candidates, and a ready-to-use first task.
- Seven UI reference images. The instrument rail guides optional Flow; older Flow images document interaction examples. The default workspace follows the user's CAD-convention feedback. Users can replace the entire visual composition.
- An illustrative multi-file project fixture with stable identities, graph branches, repeated part instances, presentation state, two UI compositions, and separate styling.
- A manifest and [package validation report](VALIDATION.md).

## Illustrative workflow

Open a small assembly, isolate a support, select an edge, edit its fillet radius in the instrument rail, inspect a real evaluated ghost preview, then Apply or Cancel. Save and rebuild in a fresh process. The workflow must remain usable with a trackpad and keyboard.

The reference render is a design target and the example JSON is a proposed fixture format. Demo documents are explicitly opt-in; a new document starts empty. The example workflow does not bound the required mechanical CAD feature set or authorize probe/manual-test runs.

## Reading order

| File | Purpose |
|---|---|
| [00-decisions](docs/00-decisions.md) | What has already been chosen |
| [01-product-spec](docs/01-product-spec.md) | Complete product requirements |
| [02-architecture](docs/02-architecture.md) | Boundaries and responsibilities |
| [03-data-contracts](docs/03-data-contracts.md) | Stable identities and native source |
| [04-preview-and-undo](docs/04-preview-and-undo.md) | Draft evaluation and commit safety |
| [05-instrument-rail](docs/05-instrument-rail.md) | Default layout/theme, typography, and rendering |
| [06-trackpad](docs/06-trackpad.md) | Gestures and input routing |
| [07-first-build](docs/07-first-build.md) | Milestones and acceptance criteria |
| [08-validation](docs/08-validation.md) | Correctness and usability checks |
| [09-dependencies](docs/09-dependencies.md) | Verified sources and starting versions |
| [10-open-decisions](docs/10-open-decisions.md) | Unresolved choices and how to resolve them |
| [11-replaceability-and-presentation](docs/11-replaceability-and-presentation.md) | Replaceable subsystems, semantic UI, user layout/style resources, and policy against code tests |

The original application code is intended for WTFPL. Dependencies and third-party assets retain their own licenses and notices. The generated UI images are visual references; production fonts, textures, icons, and environment maps need their own recorded provenance.
