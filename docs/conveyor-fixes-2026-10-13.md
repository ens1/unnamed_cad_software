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

Validation: `cargo fmt --all`, `cargo check -p cad-desktop`, and `cargo clippy -p cad-desktop -- -D warnings` passed for this earlier navigation checkpoint. It did not include an application bundle rebuild or interactive acceptance; final integration evidence is recorded below.

## Follow-through on the review

| Finding | Implementation |
| --- | --- |
| P1-01 Editor responsiveness | Normalized field reads and a complete operation semantic projection are cached by source, evaluation, editor and selection identity. Modeling actions reconstruct their current descriptor at dispatch. |
| P1-02 Drawing responsiveness | Persistent off-thread projection/display preparation, coalesced requests, stale-result rejection, cached paths and viewport culling. |
| P1-03 Selection/editing scope | Single-click occurrence selection, explicit definition editing, occurrence-aware Move/Rotate and pattern seeds, breadcrumbs and visible scope. |
| P1-04 Context picking | Ghost context excluded by default; explicit reference picking; active-occurrence priority; candidate menu with occurrence/entity labels and a native boundary thumbnail; Tab consumes its key. |
| P1-05 Assembly tree | Cached hierarchy, pattern groups, natural sorting, collapsed subassemblies, virtualized rows, search and source-instance renaming. Search/name controls also have semantic value adapters. |
| P1-06 Task rail | One operation header and sticky confirmation; primary fields first; compact/searchable references; optional history, help and diagnostics. See [task-panel notes](conveyor-task-panel-fixes-2026-10-13.md). |
| P1-07 Existing-bore counterbore | Certified coincident circular carrier/endpoint partitions feed the existing exact layered material subtraction. Ordinary crossing checks and final native validation remain strict. See [construction notes](conveyor-counterbore-fix-2026-10-13.md). |
| P1-08 Pattern directions | Associative sketch-line, datum-axis and straight model-edge directions, spacing, reverse and second-direction grids for body/component patterns; native evaluated on-model guides. |
| P2-01 Stable input regions | Fixed task-status space; Smart Dimension retains its opening viewport anchor while the solved geometry moves. |
| P2-02 Completion controls | Short, content-fitting defaults; one completion group; theme appearance preview in Settings and presentation editing; explicit user shape/size choices preserved. |
| P2-03 Required inputs | Neutral next-action prompts for edges, structural member paths, draft faces, loft sections and sweep paths; native evaluation still gates Apply. |
| P2-04 Diagnostics | One concise task message with optional complete diagnostics, replacing repeated native viewport/footer error text. User CSS can explicitly restore a preview status overlay. |
| P2-05 Origin inference | Snap Entities saves persistent origin coincidence and shows the inference; Alt bypasses it. |
| P2-06 Coordinate intent | Move seed labels and driving coordinate/origin-axis actions are distinct; 3D driving-coordinate fields state their intent. |
| P2-07 Numeric display | Document units/precision govern inactive quantity, cursor and point display. Editing retains exact authored expressions and f64 seeds. |
| P2-08 Picking small geometry | Screen-point ranking, endpoint/handle arbitration, sketch/model filters, sketch/model candidate choosers and fit-selection, including component/subassembly framing. See [sketch notes](conveyor-sketch-fixes-2026-10-13.md). |
| P2-09 Save As | One editor contains human name, destination and optional standalone part renaming. Browse changes the destination alone; definition renaming defaults off for saved copies. |
| P2-10 Sheet states | Formed/Flat controls preview one persistent fabrication-state feature through the normal validated draft. Explicit selected-sheet flat DXF export names its fabrication state independently of the viewport. |
| P2-11 Repeated highlights | Preview contract 3 references current native meshes for added occurrences and shares each removed definition mesh once. One bounded display payload evicts excess unique geometry/metadata and identifies omitted entries; validated model geometry stays available. |
| P2-12 Grain/context competition | New clear-anodized defaults use 0.006 mm height range; the filtered height gradient influences directional reflection. Existing authored finish values remain intact. Ghost boundaries have a quieter default and a separate styleable state. |

These entries describe implemented behavior and code integration, not successful interactive acceptance. The saved conveyor concepts have not been edited or completed as production designs by this implementation work. Rear supports, complete shafts/inserts, fasteners, welds, bracing, materials/mates and manufacturing deliverables remain the design tasks documented in the review. General unsupported fillet/freeform cases and other broader CAD coverage gaps are not declared closed by these fixes.

## Save, fabrication and preview contracts

Save Copy accepts a project folder or manifest destination and normalizes it to an owned project folder and `project.json`. It applies the validated rename to a staged session, writes the copy and promotes that session only after the write succeeds. A failed save retains the original committed session and retryable name/destination editor, so a later ordinary Save cannot write the proposed copy name into the original. Names and source identities do not derive from the filename. Cancellation retains the starting source and does not write a copy.

Formed/Flat modifies an existing terminal Flatten feature, or introduces one Fabrication State feature on the selected current native sheet output. Repeated switching edits that same identity instead of appending flat/formed history entries. Other feature history remains present. The change uses shared preview, OK/Cancel and undo; it is disabled during cached-history inspection and unrelated drafts. Controls resolve the current occurrence/configuration and evaluated state, rather than an incidental last edge pick. Dispatch reconstructs the current enabled descriptor and rejects a cached action from another occurrence/configuration or a replaced output. Native display data must match the complete current evaluation identity. Pinned definitions cannot be changed; configurations that redirect a suppressed base output expose flat export but require restoring the base output before shared-definition state editing. `sheet-metal-dxf:` carries a typed source/occurrence selection, and the worker checks effective configured outputs and exports that stock's evaluated flat contours/bend lines with current native validation. General imported-body unfolding remains outside this correction.

Shared occurrence highlight references are display-only data. Added occurrences reuse the evaluation's mesh dictionary; removals retain one bounded baseline mesh per key. Mesh data is admitted before cloning, shared across paths and pruned under the unique display payload bound. Only the accepted preview identity supplies these references. Geometry, export and Apply validity do not depend on display cache admission. This fixes repeated serialization against the reported highlight bound; the renderer still bakes placed GPU meshes per occurrence. General GPU instancing remains part of the broader large-assembly performance work.

## Integration evidence

The final integration passed `cargo fmt --all`, `cargo check --workspace --locked`, `cargo clippy --workspace --locked -- -D warnings`, and `git diff --check`. `./scripts/package-macos.sh` completed the workspace build, Info.plist validation and strict nested/bundle signature verification, then replaced `dist/TCAD.app`. The bundle contains distinct `TCAD`, `tcad-worker` and `tcad-cli` filenames. The existing `proc-macro-error2` 2.0.1 future-incompatibility notice remains; no new Clippy warnings were accepted.

The final native adapter fingerprint is `monstertruck/0.4.1+fc2306852a46;adapter/44;solver/tcad-native-lm/18`; preview display contract 3 and component-pattern source contract 2 record their independent changes. The existing-bore constructor was committed and pushed separately before this integration checkpoint.

No code tests, assertion probes, geometry runtime probes, manual smoke checks or UI automation were run. The user must still accept the interaction, appearance, counterbore, referenced-pattern and larger-drawing behavior in the rebuilt application.
