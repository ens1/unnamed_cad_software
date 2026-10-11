# Open decisions and how to resolve them

Resolve these with focused manual demonstrations. Preserve approved product behavior and keep implementation choices replaceable.
The stack choices are initial implementations that may be replaced through their contracts; product behavior and user ownership remain the governing requirements. Do not write code tests; collect evidence through build/lint checks, runtime diagnostics, and manual demonstrations.

| Decision | Evidence required | When |
|---|---|---|
| Pinned dependency set | Successful resolution/build, compiler support, recorded licenses | M0 |
| Editable-field toolkit | Numeric/expression entry, focus, clipboard, scaling, scrubbing, accessibility | M0 |
| Subsystem contracts/composition | Versioned boundaries, capabilities, adapter ownership, replacement/migration path | M0 onward |
| Semantic UI contract | Theme-agnostic IDs/roles, typed data, relationships, commands, accessibility | M0–M1 |
| Layout/style grammar | Arbitrary composition, selectors/cascade, variables, overrides, reload and recovery | Probe M0; implement M3 |
| Sketch solver | Permissive license; constraints, degeneracy, diagnostics, reproducibility | Select during M0; integrate M5 |
| Placement solver | Rigid transforms, common mates, consistency and solver diagnostics | Select during M0; integrate M5 |
| Input adapters | Actual event traces, precision/lifecycle, device/OS fallback behavior | M0 and every supported platform |
| Persistent reference resolver | Provenance/history behavior through parameter and topology changes | M0–M2 |
| Native source schema | Stable IDs, units, useful diffs, migrations, multi-file recovery | M1 |
| IPC encoding/bulk transfer | Cancellation/restart correctness and measured transfer costs | M2 |
| Material resources | Production asset licenses, scale, render quality and hardware cost | M2–M3 |
| Default gesture profile | Physical task completion, precision, fatigue, error rates | Before release |
| Complex graph presentation | Branching/multiple outputs, overflow, readable ports/types in default and user layouts | M3 |
| Product name | User choice before naming public repository/package | Before publication |

## Primary engineering risks

**Kernel robustness and topology history.** Passing a simple solid/fillet probe does not establish professional CAD coverage. Maintain failing fixtures for unsupported edge chains, tangencies, imports, and reference reconstruction; implement improvements where the root cause lives.

**Graph and history projection.** A vertical rail is a presentation. Shared inputs, multiple bodies, and assembly constraints remain explicit domain relationships.

**Platform input differences.** A trackpad-first promise depends on actual events and hardware usability, not mouse-wheel compatibility alone.

**UI field depth.** Large attractive numerals are easy to draw; precise expression input, focus, accessibility, and gesture ownership need evidence.

**Rendering versus authority.** Beautiful material rendering must remain separate from geometric correctness and persisted design intent.

**Replaceability versus coupling.** Documented boundaries need real ownership discipline: adapters must not leak vendor types, and consumers must not bypass contracts. Capability extensions and explicit migrations avoid hiding behavior differences behind a generic interface.

**User-defined presentation.** Moving and restyling a parameter or action must preserve its semantic identity, draft, availability, focus, and gesture behavior. Malformed layouts and hidden controls need runtime diagnostics and an independent recovery path.

## Decision record format

Record the question, considered approaches, representative fixture, measured result, chosen implementation, dependency/license impact, and remaining unsupported cases. Link code and reproducible commands.

Do not use every unresolved choice as a reason to stop the project. Implement independent foundations while collecting evidence, and bring only material product-scope changes back to the user.
