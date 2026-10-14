# Conveyor task-panel changes

Implementation against P1-06 and P2-01/02/03/04 in the conveyor usability review. These are presentation changes over the shared document, draft, command and validation paths. Saved review models and personal themes were not changed.

## Active task composition

The bundled rail now gives an operation one compact header, a scope row and one confirmation group above the scrolling inputs. Pending Changes and the repeated feature heading are removed. The confirmation group stays visible while scrolling. Native viewport confirmation yields to a composed property manager or explicit Apply control; custom layouts may still deliberately place their own semantic controls.

During editing, the dependency region defaults to 38 logical pixels. Model history is a separate optional expansion at the end of the task controls rather than preceding the current fields. Cached history inspection remains independent of source transactions. The default sizing is ordinary layout data and can be replaced by user resources.

Selected edge/member references show a small summary and at most two selected rows in the default collapsed view. Searchable expansion exposes the remaining rows. Structural sketch member rows use compact selectable controls rather than separate caption/checkbox stacks. Primary profile, size, treatment and movement fields remain open. Detailed instructions and secondary operation groups default to collapsed presentation.

Move/Rotate fields precede the component chooser. The chooser is collapsed, searchable and virtualized; primary component identity remains visible. Native duplicate completion rows are removed from movement, mates, configuration and assembly weld editors; component-pattern completion uses the same sticky group. Move constraints and requested-goal discrepancies remain in diagnostics, with a concise limited-movement state when mates restrict a requested goal.

## Status and diagnostics

Task status reserves a fixed-height region. Evaluating, valid, required-input and failure messages do not move the primary fields. Chamfer/fillet without edges, structural members without paths, draft without faces, loft without two sections and sweep without a path show neutral next-action prompts. Apply remains gated by the existing current-source/current-evaluation checks; the presentation does not authorize incomplete geometry.

A failure has one concise task message. Full kernel, source and reference detail is retained in an initially closed Diagnostics expansion and in independent semantic descriptors. Native viewport and footer chrome no longer repeat the task's long diagnostic. Explicitly expanded source editors still expose their source-specific details. Confirmation labels are compact OK, Finish, Insert or Apply, with task context in the header. Bundled confirmation styles retain auto width, rectangular defaults and wrapping rows. Explicit user width, height and shape overrides are preserved; clipped controls retain complete accessible labels and hover text. Settings' Themes category and Layout and theme editor now provide completion-control appearance previews.

## Replaceable presentation and bounded editor work

Task header/status, history visibility, diagnostics visibility, reference expansion/search and completion previews publish semantic identities, roles, state and view-only actions. Search is a registered presentation value adapter; it does not enter CAD source or undo history. Existing source fields and modeling actions retain their ordinary validated command paths. Themes may move, hide, style or replace the default regions and reference controls.

The operation editor now caches its complete semantic projection for the active document/draft, editor, evaluation identities, selection and field-error state. It retains one disposable projection; camera navigation reuses topology choices and provenance rather than rebuilding them each frame. Edge-picking mode, checkpoint/draft evaluation and selected contact position are included in invalidation. Mutation dispatch bypasses the display cache and resolves a fresh current descriptor. This extends the earlier normalized field-read cache.

## Verification

`cargo fmt --all`, `cargo check -p cad-desktop` and `cargo clippy -p cad-desktop -- -D warnings` passed for the implementation checkpoint. `git diff --check` passed. Earlier compilation/lint attempts exposed integration errors in concurrently edited files; those were resolved before the successful pass.

No code tests, assertion probes, manual smoke checks or UI automation were run. Compilation and linting establish code integration, not interactive acceptance. The actual edited rail and existing personal theme still require the user's acceptance in a rebuilt application.
