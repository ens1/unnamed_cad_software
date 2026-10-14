# Modeling, search, and display tolerance separation

Implemented source changes on 2026-10-14. Interactive acceptance remains with the user; compilation does not establish geometry acceptance or robustness.

## Application-owned contract

`cad_core::tolerance::Modeling`, contract 1, records two independent canonical millimeter budgets:

- `linear_mm`: native boundary/carrier residual admission, default `0.000001` mm, matching the pinned kernel's numerical precision floor. Allowed range `0.000001–0.001` mm. Existing native constructors and consistency checks may enforce stricter limits.
- `boolean_search_mm`: native intersection/Boolean discovery and subdivision resolution, default `0.05` mm. Allowed range `0.0001–1` mm, no smaller than the residual budget. A discovery mesh never establishes final B-rep accuracy.

`ModelSettings::display_tolerance_mm` is a separate disposable viewport/export chord budget, default `0.15` mm. Allowed range `0.000001–1` mm. Viewport automatic refinement remains a separate, view-dependent display contract.

Model settings contract 2 reads contract 1 settings and the legacy `evaluation_tolerance_mm` field. The old value migrates only to display quality; modeling policy receives independent defaults. Personal new-model preferences use the same migration. Existing filenames, source IDs, and native geometry definitions are preserved. Model settings are still ordinary undoable document edits.

## Construction and transport

`Evaluator::build(document, root)` has no display tolerance argument. It validates and configures the saved modeling policy, then uses only `boolean_search_mm` for construction, configured mate geometry, sheet metadata rebuilds, and manufacturing operations. Its mechanical cache fingerprint includes the entire modeling policy and excludes display quality. History/display mesh caches retain their separate chord budget keys.

Worker protocol 2 calls its evaluate/export/release scalar `display_tolerance_mm`; legacy JSON `tolerance` is accepted as an alias, while the protocol identity rejects incompatible workers. Modeling policy travels in the fingerprinted document. Display mesh request contract 2 additionally binds its modeling policy to the document and backend identity. A cache miss constructs native geometry with the same saved policy, then meshes using the independent display budget.

Assembly weld component rebuilds derive policy from each scoped document; weld meshes use the supplied display budget. Sheet metadata uses modeling search resolution. Exact operation-preview delta Booleans use the adapter's modeling search budget; their displayed meshes use chord tolerance. Assembly-inspection definitions retain their explicit saved per-inspection search expression, with the UI relabeled “Boolean search resolution”; newly created inspections seed it from the modeling policy.

Evaluation responses report `Accuracy { version, modeling, display_mm }`. Release manifest schema 2 records that complete accuracy record. STEP, drawing, STL, flat-pattern, profile, and other exports all construct native geometry using the saved model policy; the export scalar affects display/meshed output. CLI `--display-tolerance` makes that distinction explicit and retains `--tolerance` as an alias.

## Presentation

Model geometry and application new-model defaults expose three separate semantic settings controls: boundary residual tolerance, Boolean search resolution, and display chord tolerance. Help explains their separate meanings. Numeric controls retain up to nine fractional places and use appropriate residual/search steps. Layout and styling remain user-defined over the existing settings descriptor/setter contract.

## Limits and evidence

This change separates policy and plumbing; it does not make previously unsupported topology or carriers valid. The adapter admits available native face trims and supported intersection continuation at the saved residual before recording a feature result. Meshing, inspection, assembly deposition and STEP exchange use the same native trim boundary. Existing fixed kernel numerical checks remain stricter where applicable. Proof conditioning and resource budgets are not modeling tolerance, and changing this policy does not waive validity certificates.

No tests, assertion probes, or manual UI checks were written or run. Compilation, formatting, and lint evidence is reported with the integrated checkpoint.
