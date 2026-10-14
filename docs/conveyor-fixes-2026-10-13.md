# Conveyor review fixes

This records implementation against the [conveyor usability review](conveyor-project-usability-review-2026-10-13.md). Compilation and linting are recorded separately from interactive acceptance. No code tests or manual smoke checks are used.

## Editor and drawing responsiveness

Operation field reads now use one normalized value projection per document, draft edit and editor identity. They do not rebuild every topology choice or encode provenance for each displayed value. Apply availability skips topology-choice construction when there is no invalid operation field text. Mutations still resolve the current semantic descriptor and validate identity and availability at dispatch.

Paper projection, interaction-scene construction and spline/dash display preparation run on a persistent display worker. Its mailbox retains at most one running request, one replacement request and one completion. New requests supersede previous results; document generation, draft edit, sheet and page identify the accepted result. Cancel and navigation do not wait for this worker. Preparation errors return to the UI, including unexpected worker panics.

Paper display uses cached paths, viewport culling and one path shape per solid polyline rather than resampling and emitting each segment separately every frame. The original rational curves remain available to drawing references and exact export. Display data is disposable and does not become authoritative geometry.

Validation: `cargo fmt --all`, `cargo check -p cad-desktop`, and `cargo clippy -p cad-desktop -- -D warnings` passed. Interactive acceptance remains outstanding.

## Assembly navigation and picking

The native assembly tree now follows the source assembly hierarchy, groups generated occurrences under their component patterns, and sorts names naturally. Its source projection is cached by document generation and draft edit, with search, collapsible groups and virtualized rows. Occurrence paths identify rows even when several occurrences have the same display name. Semantic descriptors expose hierarchy, identity and actions independently of the native row layout and style.

A single tree click selects an occurrence and outlines its geometry without entering the part definition. Double-click or Edit definition enters editing explicitly. Move and Rotate can use this occurrence selection; component-pattern creation prefers its selected seed in the target assembly. Context actions include rename and grounding, with the current-operation guard retained. Clicking empty viewport space clears the occurrence selection, and loading another document resets tree presentation state.

Normal picking excludes ghosted context and prioritizes the active occurrence. Pick references explicitly includes visible surrounding components; feature tools still enforce their input-body and occurrence restrictions. Candidate cycling consumes Tab and retains viewport focus.

Validation: `cargo fmt --all`, `cargo check -p cad-desktop`, and `cargo clippy -p cad-desktop -- -D warnings` passed for this checkpoint. No interactive acceptance or application bundle rebuild was performed.

## Remaining implementation

This is a source checkpoint, not completion of the conveyor findings. Compact rail composition, stable task fields, reference-driven linear patterns, coincident counterbore construction, the remaining selection refinements and the report's P2 polish items remain outstanding. The new responsiveness and navigation behavior also awaits user acceptance in a rebuilt application.
