# Modeling reliability and workflow cleanup — October 12, 2026

This implementation follows the [saved-part usability review](usability-review-2026-10-12.md). The original review and its three saved parts remain unchanged. The changes below address the observed failure paths and UI friction; compilation and linting do not establish interactive acceptance.

## Modeling failures

| Review | Implemented change | Supported scope and limits |
| --- | --- | --- |
| R1: formed-sheet cuts exhaust ancestry | Store complete readable constructor ancestry in canonical indexed source closures, deduplicate repeated descriptors, and retain legacy source-reference matching. Diagnostics identify the exhausted budget. | Existing work/storage limits remain enforced. This changes provenance representation, not unrelated curved Boolean support. See [source closures](topology-source-closures.md). |
| R2: further counterbores/pockets report open cap arrangements | Classify shared circular endpoints with one half-open native endpoint rule; use exact finite line/circle side predicates and identify the actual Boolean transition in errors. Add bounded local planar-cap insertion for downstream holes/pockets on stock with distant bevels and blends. | Complete entry/exit footprints and native separation from every untouched carrier/void must pass. Existing convex/layered constructors retain priority. Intersecting arbitrary curved carriers remain a separate limitation. See [native prismatic Booleans](native-prismatic-booleans.md) and [local pockets](native-local-pockets.md). |
| R3: adjoining planar chamfers reject an elevated boss | Construct bounded local simultaneous planar setbacks and corner intersections; prove unrelated geometry lies outside each local removal prism. Use one aggregate proof budget. | Native straight convex edges with resolved planar neighborhoods and terminals. Arbitrary curved transitions, ambiguous ancestry and unresolved corner junctions remain unsupported. See [edge chamfers](edge-chamfers.md). |

## Everyday workflow changes

- **Circular rims:** Fillet and Chamfer picks prefer a complete native circular rim. A visible selection-mode control retains individual segments when needed. Explicit multiselections remain intact; rectangular and branching chains still use their existing explicit expansion actions.
- **Material previews:** Common additive/subtractive differences use the native layered Boolean path. Native planar chamfer clips retain their removed pieces, and complete circular rim constructors close exact cap/wall/blend bands. Current before/after native identities guard reuse. Bounded display-only BSP remains a fallback for other supported geometry. Status near confirmation and beside the geometry identifies pending, retained, approximate, partial and unavailable highlights; invalid edits identify the previous valid display. A display approximation never validates source geometry or enables Apply.
- **Sketch controls:** Active dimension/member inputs precede the relation browser. Help, filters, modifiers and secondary controls fold below them. Evaluated driven circle/ellipse sizes and coordinates use model display units and precision. Authored expression text and unedited f64 coordinates retain their source values.
- **Completion controls:** Default themes fit full Finish Sketch / Finish Positions labels with natural rectangular widths. Completion roles and classes remain available to arbitrary user styles.
- **Framing:** Normal To preserves the useful camera target projected onto the sketch plane and expands its scale only as required. Fit Active Sketch, Fit Support Face and Fit Editing Part provide explicit alternatives. Hole-position editing frames its own part occurrence.
- **Snapping:** A separate entity-snap toggle and a visible candidate precede placement. Alt bypasses entity and grid snapping; independent first-click intent survives subsequent gesture clicks. Inferred corners do not silently join unrelated points.
- **Fabrication sources:** Presets prefer explicitly selected/newest compatible sketches and separate section/path intent. A previous flange is not silently treated as an explicit terminal selection. Existing authored source fields remain intact.
- **Body inventory:** Solid/surface counts come from native body containers, independently of output features and boundary shells. The tree groups entries by output feature and labels older unavailable count metadata as outputs.
- **Cut lists:** Section names use current Part / Sketch and library labels. Solved analytic extents, outside diameters and proved uniform tube walls accompany those names; stable source references remain separate in metadata and CSV. Names and display sizes do not affect stock equivalence or item identity. See [fabrication source labels](fabrication-source-labels.md) and [section sizes](section-size.md).
- **Overlay clutter:** Auto reveals selected/active sketches and datums. Show All, Hide, and independent X-ray choices persist in presentation state and semantic actions. Visible-depth overlays use bounded disposable-mesh occlusion; exhaustion omits optional decoration. Occlusion is a display approximation, not analytic visibility.
- **Curved shading:** Native face UV/carrier normals and corrected per-face fallbacks retain hard CAD edges. Inverse-transpose normal mapping and bounded bump-variation filtering preserve the imported grayscale grain source, tile scale and depth. Zoom-dependent tessellation runs separately from model evaluation. See [visual finishes](visual-finishes.md) and [viewport detail](viewport-display-detail.md).
- **File/context polish:** Save As uses a Save file dialog; successful save feedback includes the path and survives automatic presentation reload. Rename Document offers optional standalone part-name synchronization through the same validated draft/undo path, preserving IDs and folder paths. Standalone parts omit assembly-isolation wording/controls. Texture palette actions identify their finish as well as their map slot.

## Validation and review

Backend cache identity is `monstertruck/0.4.1+fc2306852a46;adapter/40;solver/tcad-native-lm/16`. Saved source schema remains 1 with additive optional display metadata. Source closures, native body counts and fabrication source labels are independent application-owned versioned contracts.

Final checks passed: `cargo check --workspace --locked`, `cargo fmt --all --check`, `cargo clippy --workspace --lib --bins --locked -- -D warnings`, `git diff --check`, and `scripts/package-macos.sh` (workspace build, property-list validation and nested/bundle signature verification). The rebuilt local bundle is `dist/TCAD.app`. An independent static review also checked local cut admission, blind/through cap ordering, void preservation and cropped-tool provenance. No code tests, geometry probes, manual smoke runs or interactive acceptance were performed. Rust compilation does not exercise GPU shader pipeline creation. The existing `proc-macro-error2` dependency reports a future-compatibility notice.

The saved review files retain their original accepted features and rejected-operation repro sketches. They can be reopened for user acceptance of the original sheet cut, mounting counterbores/pockets and adjoining chamfer. Unresolved mixed-carrier fillet/chamfer junctions and unsupported intersecting curved Boolean cases remain broader kernel limitations.

## Follow-up completion work

The earlier checkpoint did not close every review finding. This follow-up adds:

- **Weld authoring:** complete two-face coplanar butt/miter contact, bounded unique native support inference, explicit seam/support picks and highlights, and profile/end-trim recovery. The exact rejected weld was not saved; no claim is made that a physically invalid free edge now becomes a valid joint. See [native weld contacts](native-weld-joint-contacts.md).
- **Fabrication sources:** separate viewport pick intents for original base perimeter and available flange terminal, using native adjacent-face constructor ancestry. Source guides highlight the chosen attachment, section and path independently of material previews. Flange faces and thickness edges no longer silently imply a terminal.
- **Quantities:** compact finite literals with exact authored values restored on focus, shared semantic presentation metadata, retained invalid input and confirmation gates. Compound expressions remain authored. See [quantity presentation](quantity-presentation.md).
- **Display:** separate persistent zoom-dependent native tessellation, bounded display budgets, identity/generation/root rejection of stale results, inverse-transpose instance normals and continuous projected depth clipping of optional overlays. Geometry tolerances and Apply validation remain independent. See [viewport display detail](viewport-display-detail.md).
- **Names and saves:** first Save and Save As choose a project-folder destination, then propose its chosen name in editable document/part fields before confirming the copy. The chosen destination and current saved location have independent semantic controls. Cancel before confirmation does not rename source or write a copy. Palette results distinguish editor targets, finishes and named camera actions.
- **Cut-list sections:** configured solved analytic extents, diameters and proved uniform tube walls accompany named profile references. Source IDs, grouping and quantity remain unchanged; display measurements never come from render meshes.
- **Contour inspection:** compact actions select affected sketch members for branched contours, duplicate/collapsed lines and exact ambiguous endpoint clusters. These view-only actions retain source, undo and dimension checkpoints; current-scope checks reject stale actions. See [contour inspection](sketch-contour-inspection.md).
- **Preview feedback:** a user-styleable status beside the viewport geometry identifies retained, invalid, partial and approximate previews.

## Finding-by-finding implementation coverage

This records code and supported recovery paths, not a repeated UI run. Interactive acceptance remains pending for every change; the original report and saved parts are preserved.

| Finding | Implemented response |
| --- | --- |
| R1 | Bounded canonical source closures and specific provenance-budget diagnostics. |
| R2 | Exact endpoint arrangements plus local cuts on covered planar stock, preserving distant native blends and voids. |
| R3 | Simultaneous bounded planar chamfer corners with native material-removal captures. |
| R4 | Complete circular-rim intent by default, with explicit individual-segment mode. |
| R5 | Native common-feature deltas, constructor fillet/chamfer patches and retained/error labels beside geometry. |
| R6 | Active sketch inputs first, secondary controls folded and readable completion buttons. |
| R7 | Framing-preserving Normal To plus explicit sketch/support/part fit targets. |
| R8 | Visible snap candidates, independent contour placement and selectable contour-issue diagnostics. |
| R9 | Native base/terminal pick intent, named compatible sources and independent source guides. |
| R10 | Native solid/surface container counts; unavailable legacy metadata is described as outputs. |
| R11 | Named analytic section sizes, naming within Save As and visible saved project location. |
| R12 | Compact quantity presentation, evaluated driven sizes and exact source on focus. |
| R13 | Explicit Auto/All/Hidden/X-ray modes and continuously clipped depth overlays. |
| R14 | Grain/normal filtering, occurrence normal transforms and bounded zoom refinement. |
| R15 | Standalone context cleanup, save-oriented named-copy flow and persistent path feedback. |
| R16 | Target-qualified palette actions and relevant result ranking. |
| Rejected weld | Explicit seam/support picking and highlights, bounded contact inference, two-face butt/miter joints and specific recovery actions. |

The unsaved rejected weld cannot be reconstructed exactly. The retained frame includes physical end gaps, which these changes do not bridge automatically. A valid joint still needs actual supporting stock and a complete native contact proof. General curved fillet junctions, intersecting freeform cuts and broader mechanical-CAD gaps remain in [the coverage inventory](mechanical-cad-coverage.md); they were not demonstrated failures in this session.
