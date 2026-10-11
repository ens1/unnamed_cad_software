# Historical foundation backlog

The current scope is the complete application with [SolidWorks mechanical CAD as its minimum](../../docs/mechanical-cad-coverage.md). The repository's [root instructions](../../AGENTS.md) supersede the milestone-first, slice, probe, and manual-demonstration workflow below. This table preserves the original foundation plan as background; it is not the current delivery boundary or an authorization to run tests/probes. Compile, format, and lint; leave interactive acceptance to the user.

Historical milestones and acceptance criteria are detailed in [the first build](../docs/07-first-build.md).

| ID | Milestone | Task | Acceptance |
|---|---|---|---|
| B01 | M0 | Resolve stack and licenses | Pinned toolchain, lockfile, recorded features/licenses |
| B02 | M0 | Geometry and STEP probe | Real solid/cut, valid tessellation, toleranced exchange check |
| B03 | M0 | Fillet capability probe | Two valid radii, invalid case diagnostic, saved manual demonstration fixture |
| B04 | M0 | Provenance/picking probe | Triangle and edge hits trace to source topology/features |
| B05 | M0 | Trackpad capture | Available-device trace, semantic routing, unverified platforms listed |
| B06 | M0 | Editable-field and presentation probe | Exact entry, safe scrubbing, same semantic binding in two resource-defined positions/styles |
| B07 | M0 | Solver assessment | Permissive candidates evaluated against explicit constraint cases |
| B08 | M1 | Domain identity and units | Stable IDs, typed ports, dimension validation, explicit instances |
| B09 | M1 | Graph/history projections | Branches and shared definitions preserved; cycles rejected |
| B10 | M1 | Native source | Canonical multi-file round trip, migration policy, recoverable saves |
| B11 | M1 | Draft/undo state machine | Preview/Cancel preserve source; Apply creates one undo entry |
| B12 | M2 | Worker lifecycle | Revision envelopes, restart handling, stale-result rejection |
| B13 | M2 | Render bridge | Real tessellation, realistic materials, source-aware picking |
| B14 | M2 | Camera/input integration | Responsive navigation during evaluation; no duplicate gestures |
| B15 | M3 | Semantic UI, user layout/style, default rail | Theme-agnostic data/actions, editable composition/style, rail and alternate layout, reload/recovery |
| B16 | M3 | Selection-first operations | Face/edge/component actions appropriate to selected geometry |
| B17 | M3 | Ghost/commit flow | Current validated preview, comparison, Apply/Cancel and keyboard parity |
| B18 | M4 | Editing scope/isolation | In-place editing, Hide/Ghost others, pins, peek, visibility restoration |
| B19 | M4 | Shared-instance behavior | Both supports preview a definition edit; placement stays per-instance |
| B20 | M5 | Constrained sketch authoring | Construct/edit a constrained part with solver diagnostics |
| B21 | M5 | Basic placement constraints | Consistent simple mates; under/overconstraint handling |
| B22 | M6 | Promoted assembly parameter | One source parameter drives linked geometry/placements atomically |
| B23 | M6 | Hardware/platform matrix | Complete trackpad workflows verified on supported platforms |
| B24 | M6 | Fresh-process reproducibility | Cacheless source rebuild, references and expected properties checked |
| B25 | M0 onward | Replaceable subsystem boundaries | Versioned contracts, explicit capabilities, adapter ownership, composition selection, migration paths |

B02–B07 follow B01 but can proceed independently where their local inputs are available. M1 foundations depend on the evidence needed to select correct domain/kernel boundaries. B15 depends on B06 and the M1/M2 editing foundations. B18/B19 depend on the preview contract and shared definition model.

Drawings, BOMs, sheet metal, weldments, molds, surfaces, configurations, exchange, releases, and semantic Git workflows are required complete-product work. Track mechanical breadth and mode/geometry gaps in the coverage inventory rather than deferring them because of this old foundation plan.

Do not write or run code tests, assertion probes, or manual smoke tests. Retain compile/format/lint checks and runtime validation; the user performs interactive acceptance. Replaceability and semantic UI separation apply to every feature throughout development.
