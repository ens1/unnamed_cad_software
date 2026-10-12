# Modeling reliability and workflow cleanup — October 12, 2026

This implementation follows the [saved-part usability review](usability-review-2026-10-12.md). The original review and its three saved parts remain unchanged. The changes below address the observed failure paths and UI friction; compilation and linting do not establish interactive acceptance.

## Modeling failures

| Review | Implemented change | Supported scope and limits |
| --- | --- | --- |
| R1: formed-sheet cuts exhaust ancestry | Store complete readable constructor ancestry in canonical indexed source closures, deduplicate repeated descriptors, and retain legacy source-reference matching. Diagnostics identify the exhausted budget. | Existing work/storage limits remain enforced. This changes provenance representation, not unrelated curved Boolean support. See [source closures](topology-source-closures.md). |
| R2: further counterbores/pockets report open cap arrangements | Classify shared circular endpoints with one half-open native endpoint rule; use exact finite line/circle side predicates and identify the actual Boolean transition in errors. | Native layered prismatic arrangements retain their existing analytic-carrier and uncertainty limits. See [native prismatic Booleans](native-prismatic-booleans.md). |
| R3: adjoining planar chamfers reject an elevated boss | Construct bounded local simultaneous planar setbacks and corner intersections; prove unrelated geometry lies outside each local removal prism. Use one aggregate proof budget. | Native straight convex edges with resolved planar neighborhoods and terminals. Arbitrary curved transitions, ambiguous ancestry and unresolved corner junctions remain unsupported. See [edge chamfers](edge-chamfers.md). |

## Everyday workflow changes

- **Circular rims:** Fillet and Chamfer picks prefer a complete native circular rim. A visible selection-mode control retains individual segments when needed. Explicit multiselections remain intact; rectangular and branching chains still use their existing explicit expansion actions.
- **Material previews:** Common additive/subtractive differences use the native layered Boolean path. Bounded display-only BSP work chooses less degenerate partitions. Status near confirmation identifies pending, retained, approximate, partial and unavailable highlights; invalid edits explicitly identify the previous valid display. A display approximation never validates source geometry or enables Apply.
- **Sketch controls:** Active dimension/member inputs precede the relation browser. Help, filters, modifiers and secondary controls fold below them. Evaluated driven circle/ellipse sizes and coordinates use model display units and precision. Authored expression text and unedited f64 coordinates retain their source values.
- **Completion controls:** Default themes fit full Finish Sketch / Finish Positions labels with natural rectangular widths. Completion roles and classes remain available to arbitrary user styles.
- **Framing:** Normal To preserves the useful camera target projected onto the sketch plane and expands its scale only as required. Fit Active Sketch, Fit Support Face and Fit Editing Part provide explicit alternatives. Hole-position editing frames its own part occurrence.
- **Snapping:** A separate entity-snap toggle and a visible candidate precede placement. Alt bypasses entity and grid snapping; independent first-click intent survives subsequent gesture clicks. Inferred corners do not silently join unrelated points.
- **Fabrication sources:** Presets prefer explicitly selected/newest compatible sketches and separate section/path intent. A previous flange is not silently treated as an explicit terminal selection. Existing authored source fields remain intact.
- **Body inventory:** Solid/surface counts come from native body containers, independently of output features and boundary shells. The tree groups entries by output feature and labels older unavailable count metadata as outputs.
- **Cut lists:** Section names use current Part / Sketch and library labels, while stable source references remain separately available in metadata and CSV. Names do not affect stock equivalence or item identity. See [fabrication source labels](fabrication-source-labels.md).
- **Overlay clutter:** Auto reveals selected/active sketches and datums. Show All, Hide, and independent X-ray choices persist in presentation state and semantic actions. Visible-depth overlays use bounded disposable-mesh occlusion; exhaustion omits optional decoration. Occlusion is a display approximation, not analytic visibility.
- **Curved shading:** Native face UV/carrier normals and corrected per-face fallbacks retain hard CAD edges. Inverse-transpose normal mapping and bounded bump-variation filtering preserve the imported grayscale grain source, tile scale and depth. Circular silhouettes still depend on model display tolerance. See [visual finishes](visual-finishes.md).
- **File/context polish:** Save As uses a Save file dialog; successful save feedback includes the path and survives automatic presentation reload. Rename Document offers optional standalone part-name synchronization through the same validated draft/undo path, preserving IDs and folder paths. Standalone parts omit assembly-isolation wording/controls. Texture palette actions identify their finish as well as their map slot.

## Validation and review

Backend cache identity is `monstertruck/0.4.1+fc2306852a46;adapter/38;solver/tcad-native-lm/16`. Saved source schema remains 1 with additive optional display metadata. Source closures, native body counts and fabrication source labels are independent application-owned versioned contracts.

Checks performed: workspace compilation, Rust formatting, strict Clippy for libraries/binaries, whitespace checks, and macOS bundle compilation/signature verification. No code tests, geometry probes, manual smoke runs or interactive acceptance were performed. Rust compilation does not exercise GPU shader pipeline creation.

The saved review files retain their original accepted features and rejected-operation repro sketches. They can be reopened for user acceptance of the original sheet cut, mounting counterbores/pockets and adjoining chamfer. Unresolved mixed-carrier fillet/chamfer junctions, unsupported curved Boolean cases, automatic zoom remeshing, and inferred weld seams without proven support contact remain separate limitations.
