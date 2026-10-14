# Conveyor review fixes

This records implementation against the [conveyor usability review](conveyor-project-usability-review-2026-10-13.md). Compilation and linting are recorded separately from interactive acceptance. No code tests or manual smoke checks are used.

## Editor and drawing responsiveness

Operation field reads now use one normalized value projection per document, draft edit and editor identity. They do not rebuild every topology choice or encode provenance for each displayed value. Apply availability skips topology-choice construction when there is no invalid operation field text. Mutations still resolve the current semantic descriptor and validate identity and availability at dispatch.

Paper projection, interaction-scene construction and spline/dash display preparation run on a persistent display worker. Its mailbox retains at most one running request, one replacement request and one completion. New requests supersede previous results; document generation, draft edit, sheet and page identify the accepted result. Cancel and navigation do not wait for this worker. Preparation errors return to the UI, including unexpected worker panics.

Paper display uses cached paths, viewport culling and one path shape per solid polyline rather than resampling and emitting each segment separately every frame. The original rational curves remain available to drawing references and exact export. Display data is disposable and does not become authoritative geometry.

Validation: `cargo fmt --all`, `cargo check -p cad-desktop`, and `cargo clippy -p cad-desktop -- -D warnings` passed. Interactive acceptance remains outstanding.

Selection scope, hierarchical navigation, rail composition, reference-driven patterns, counterbore construction and the remaining polish items are still being implemented.
