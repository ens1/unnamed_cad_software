# Interactive modeling review — 2026-10-12

Three independent parts were created through the native TCAD UI, saved, and reopened with valid geometry and their feature histories. Basic sketch-to-part, formed sheet, and mixed-section structural workflows are usable. Ordinary fabrication edits still encounter geometry failures: sheet holes, mounting counterbores, shallow pockets, and adjoining-edge chamfers were blocked.

The UI has recognizable CAD commands and useful face/edge context actions. Its main usability problems are selection granularity, camera framing, crowded sketch controls, and weak preview feedback. These add friction even when an operation succeeds.

This was the user-requested interactive modeling session. No code tests, assertion harnesses, or automated geometry probes were written or run. No application or kernel implementation was changed. Failed operations were recorded and canceled; the saved parts contain only accepted definitions.

Repository baseline: `c863645`. Packaged application source recorded at the start: `0eb8450`; application version 0.1.0. Kernel identity: `monstertruck/0.4.1+fc2306852a46;adapter/36;solver/tcad-native-lm/16`. The session began around 09:36 CDT on October 12, 2026.

## Review files

In TCAD, use **File → Open** and select the relevant `project.json`. Keep its adjacent `parts` and `assemblies` folders together. Each project contains one independent Part1; the directory names distinguish the review parts.

| Project | Saved result | Additional artifact | Reopen result |
| --- | --- | --- | --- |
| [Sheet-metal channel](/Users/tyler/Software/Personal/tcad/models/ui-review-2026-10-12/sheet-metal-channel/project.json) | Formed channel with two opposite flanges and an open hem | [Flat-pattern DXF](/Users/tyler/Software/Personal/tcad/models/ui-review-2026-10-12/sheet-metal-channel/flat-pattern.dxf) | Valid; all eight history entries present |
| [Weldment frame](/Users/tyler/Software/Personal/tcad/models/ui-review-2026-10-12/weldment-frame/project.json) | Six members in three section groups | [Cut-list CSV](/Users/tyler/Software/Personal/tcad/models/ui-review-2026-10-12/weldment-frame/cutlist.csv) | Valid; all three groups and six sketches present |
| [Solid mount](/Users/tyler/Software/Personal/tcad/models/ui-review-2026-10-12/solid-mount/project.json) | Plate, merged boss, through bore, chamfer, and full-rim fillet | Retained two-contour pocket sketch for reproduction | Valid; chamfer and fillet visible |

The DXF export reported its destination successfully. The CSV contains six member rows. Neither export was imported into another CAD application in this session. TCAD is left showing the reopened solid mount in isometric view.

## What was built

### Sheet-metal channel

- Top-plane rectangle, Smart Dimension width 150 mm and height 80 mm.
- Base Flange / Tab1: thickness 2 mm, inside radius 2 mm, K factor 0.4.
- Edge Flange1: 30 mm at 90° on a long base edge.
- Edge Flange2: 20 mm at 90° on the opposite long base edge.
- Open Hem1: 10 mm return and 2 mm clear gap on the second flange's terminal.
- Sketch2: two Ø6 mm circles at native coordinates (-20, -40) and (-131, -40).
- Flat Pattern1 applied, DXF exported, then Formed Sheet1 applied and saved.

All formed/flat operations above validated and applied. **The two circles are uncut sketch geometry, not holes.** Sheet Normal Cut / Relief failed and was canceled. The unused sketch remains available for reproduction. Its blue overlays can appear over the front flange even though the circles lie on the original blank plane.

### Weldment frame

- Sketch1: centered square-tube section, 20 × 20 mm outside and 16 × 16 mm inside.
- Sketch2: a driving-dimension 200 × 120 mm rectangular centerline path.
- Structural Member Group1: all four path members, miter corners, zero gap.
- Sketch3: a round section driven by a Ø12 mm Smart Dimension.
- Sketch4: a 207 mm straight path; Group2 uses 11 mm start shortening and 17 mm finish shortening, giving a 179 mm round crossbar.
- Sketch5: centered 30 × 4 mm flat-bar section; width has a driving dimension.
- Sketch6: a 175 mm straight path; Group3 appends the flat bar to the frame.
- A named camera view, Weldment Work, was authored and saved.

The mixed sections, appended groups, four mitered tubes, and shortened round bar all produced valid previews and applied. The cut list reports tube lengths 200, 200, 120, and 120 mm with 45° end angles; the crossbars are 179 and 175 mm. These are exported values, not independently checked manufacturing lengths.

This is a workflow review model, not a fully constrained fabrication design. Several sketches retain positional freedom. The frame path moved slightly from the origin while being dimensioned, and manual flat-bar placement leaves small end gaps. Automatic coping, joint contact verification, Trim / Extend, end caps, and gussets were not exercised.

A visible fillet weld was attempted from a model-edge selection. The inferred support contact failed validation, so no bead was committed. This demonstrates that the selected seam was rejected; it does not establish that all weld-bead geometries are unsupported.

### Solid mount

- Sketch1: centered 120 × 80 mm rectangle with driving width/height dimensions.
- Boss-Extrude1: 12 mm plate thickness.
- Sketch2: Ø40 mm circle directly on the plate's top face.
- Boss-Extrude2: 25 mm, Merge Result; the boss and plate merged successfully.
- Sketch3 / Drilled Holes1: central Ø20 mm Through All bore from the boss top.
- Chamfer1: 2 mm equal-distance treatment on one 120 mm plate edge.
- Fillet1: 3 mm on the complete outer boss top rim, selected as four circular segments with Select complete rim / loop.

Four mounting counterbores were attempted and canceled. Two 20 × 30 mm rectangular pocket contours were then drawn on the plate top as Sketch4; their 4 mm cut also failed. **The final part has no mounting counterbores or pockets.** Sketch4 is retained and visibly outlined in blue. Chamfer1 and Fillet1 are actual applied geometry, and both survived reopening.

## Modeling blockers and reproduction

### R1 — P1: sheet-cut ancestry limit on a small formed part

After the two flanges and open hem, use Sketch2's two circles with Sheet Normal Cut / Relief targeting Open Hem1. The UI reports:

> Part1 / Sheet Normal Cut / Relief1: Unsupported: Topology provenance: The source ancestry exceeds its bounded topology/serialization work

OK is disabled. Cancel restores valid stock. The retained channel and circle sketch provide the source definitions for repeating this workflow.

Recommendation: investigate ancestry expansion on this short sheet history, retain bounded work, and report which budget was exhausted. Source inspection of `component_history.rs` found a shared diagnostic for work and serialization bounds (16 MiB per step, 64 MiB history, and 1,048,576 work units); this run did not identify which specific bound fired. Raising a limit alone is not a proven solution.

### R2 — P1: further subtraction after a merged boss and bore

On the plate top after Drilled Holes1, create a Counterbore position sketch with four local centers (-65, 15), (-15, 105), (-65, 105), and (-15, 15) mm. Attempt Ø5 mm holes with Ø10 mm counterbores, counterbore depth 3 mm. Both Blind 10 mm and Through All produced:

> Part1 / Counterbore1: Geometry: planar boss join: Contact cap arrangement contains an open boundary

On the same stock, use retained Sketch4 with both closed contours and a Blind 4 mm Extruded Cut. The UI recognizes two closed regions, but reports:

> Part1 / Cut-Extrude1: Geometry: planar boss join: Contact cap arrangement contains an open boundary

Both features disable OK. Cancel preserves the accepted part. The counterbore transaction also removes its temporary position sketch; its coordinates are recorded above. Sketch4 remains in the saved solid as the cut reproducer.

Recommendation: trace evaluation of the input boss/bore history and planar contact arrangements before treating this solely as a multi-contour problem. Both operations hit the same diagnostic. The responsible replay or boundary-construction stage has not been isolated by this UI run.

### R3 — P1: adjoining-edge chamfer junction

Chamfer the plate's straight 120 mm edge by 2 mm: valid. Add the adjoining 80 mm top edge by clicking it during the operation: two edges selected, then:

> Part1 / Chamfer1: Unsupported: Chamfer: This selection needs an unresolved carrier transition or corner junction; no complete native blend can be constructed for the selected edges

Removing the second edge restores validation. The supported single-edge chamfer was applied and saved. Recommendation: complete ordinary planar corner-junction treatment; until then, identify the failing junction in the viewport and keep its recovery action close to the error.

### Other rejected selections

The first fillet click selected one circular segment of the outer boss rim:

> Part1 / Fillet1: Unsupported: Circular edge treatment: A partial circular treatment currently requires a planar terminal face

Select complete rim / loop expanded this to four segments, yielding valid geometry; the 3 mm fillet then applied. This is an important selection and discoverability issue, alongside partial-edge support limitations.

The weld attempt reported:

> Part1 / Weld Bead1: Support 1 / Toe 1 / Stitch 1: Weld bead: The bead toe extends outside the selected support's trimmed boundaries; reduce size or end offsets, or replace the support

The dialog explained inside seams, support faces, leg size, and end trimming. That guidance is useful. A visual support/toe overlay and an explicit support picker would make it easier to choose a valid joint. No weld is in the saved frame.

## UI usability and polish recommendations

| ID | Priority | Observation | Recommended change |
| --- | --- | --- | --- |
| R4 | P2 | An ordinary circular-rim click selected one segment; four were required for a successful fillet. | Make full rim versus individual segment a clear selection intent, with a discoverable default and highlighted extent. Avoid surfacing internal segmentation as a surprise. |
| R5 | P2 | Merged boss, bore, chamfer, fillet, and some member previews said Localized highlight unavailable. Failed edits sometimes retained a last valid preview. | Provide reliable added/removed material feedback, and visibly label stale previews beside the geometry and current error. |
| R6 | P2 | General relations, filters, diagnostics, and help consume much of the left sketch manager before member/coordinate controls. Finish Sketch and Finish Positions labels are clipped inside circular buttons. | Put active-tool inputs first, collapse secondary diagnostics, and use compact completion controls whose shape accommodates the label. Implement density through semantic presentation/theme contracts. |
| R7 | P2 | Normal To reset the view to the plane origin; off-origin sheet/frame stock moved outside the viewport. Hole placement zoomed so tightly onto the boss that much of the plate was clipped. Fit recovered it. | Preserve useful framing during alignment and provide an intentional fit target: active sketch, support face, or whole part. |
| R8 | P2 | The inner tube section initially snapped to outer corners at low zoom, creating a branching/duplicate contour error. Undo Sketch Change and a closer zoom recovered it. | Show the proposed snap target clearly and allow fine independent contour placement without accidental merging. Link duplicate/branch diagnostics to highlighted members. |
| R9 | P2 | Edge Flange2 defaulted to Flange1's terminal; the opposite base edge required changing attachment mode and reading numeric endpoints. New structural groups defaulted to Sketch1 as the section. | Prefer explicit semantic source picks and viewport highlights. Distinguish chain-flange from base-edge intent; use the selected/recent compatible section when appropriate. |
| R10 | P2 | Solid Bodies (1) is displayed for a six-member frame. Source inspection shows this label counts output features. | Count evaluated native bodies, expose individual member selection/visibility, or label the existing count as outputs. |
| R11 | P2 | Cut-list Section cells contain definition/feature UUID references; each project still presents as Part1 despite descriptive save folders. | Resolve section names and sizes in the cut list. Make document naming part of Save As and show the active project path/name. |
| R12 | P2 | Fields exposed values such as 77.99999999999878 mm and long near-180°/90° camera angles. A driven Ø12 circle's disabled source radius still showed 8 mm. | Format ordinary quantities to useful precision while retaining f64 authority. Show evaluated driving sizes, with raw source values confined to Advanced Source. |
| R13 | P2 | Blue unused sketches draw over stock and can resemble geometry on another face. Large selected plane outlines and feature callouts obscure the model. | Provide obvious sketch/plane visibility controls and consistent depth-aware overlays, with deliberate x-ray editing modes. |
| R14 | P2 | Clear-anodized cylinder surfaces show strong repetitive streaking, and circular outlines visibly facet when enlarged. | Refine default grain scale, bump/roughness filtering, lighting, and zoom-sensitive display tessellation. Preserve exact modeling authority separately from display resolution. |
| R15 | P3 | A fresh standalone Part1 retains Editing in place, Ghost others, and Finish editing controls. Save As uses an Open folder dialog, and save feedback is sometimes replaced by Presentation reloaded. | Clarify standalone versus assembly-edit context. Use save-oriented dialog labels and persistent, specific save confirmation. |
| R16 | P3 | The command palette showed repeated generic labels for target-dependent appearance actions. | Include the target finish/resource in each action label and rank context-relevant actions first. |

Face clicks did highlight the selected boundary and expose relevant actions. Edge clicks exposed Fillet and Chamfer. Selection during the chamfer operation added another edge directly and updated validation. These are useful behaviors to retain while tightening the interface.

## Feature completeness demonstrated by this run

| Workflow | Demonstrated result | Limit of the evidence |
| --- | --- | --- |
| Basic sketches and dimensions | Rectangles, nested contours, circles, open paths, driving width/height/diameter, and sketch Undo worked. | Several profiles remain underconstrained; no broad solver-completeness claim. |
| Sheet metal | Base tab, two flanges, open hem, flatten, refold, and DXF export succeeded. | Normal cut failed; complex bends, corner relief, conversion, and forming tools were not exercised. |
| Structural design | Six members, three sections, miter corners, appended groups, manual shortening, and cut-list export succeeded. | Automatic coping, Trim / Extend, caps, gussets, and library-profile insertion were not exercised. |
| Visible welds | Seam/support UI and specific contact validation were reached. | Selected joint rejected; no successful bead created in this run. |
| Solid modeling | Base extrusion, face-attached merged boss, through bore, single-edge chamfer, and full-rim fillet succeeded. | Downstream counterbores/pockets and adjoining-edge chamfer failed. |
| Local project persistence | Each native project saved and reopened with valid geometry and history. | Cross-version migration, crash recovery, and external CAD exchange were not exercised. |

Assemblies/mates, production drawings, general lofts/sweeps/surfacing, configurations, mold workflows, and theme import/styleability were not reviewed in this session. Existing implementation or command names are not evidence of complete workflows. Broader coverage remains tracked in `docs/mechanical-cad-coverage.md`.

## Recommended next work

1. Address R1–R3 first: these prevented ordinary fabrication features from being committed. Keep the saved source histories as concrete reproducers.
2. Improve rim selection, operation highlights, sketch-manager density, and camera framing (R4–R7). These immediately reduce friction in supported tools.
3. Improve source picking, native body/member visibility, evaluated quantity display, and human-readable cut lists (R8–R13).
4. Refine rendering and context/save polish (R14–R16), then conduct a separate assembly/drawing session using these saved parts.

## Environment and automation qualifications

The Mac locked twice and was manually unlocked by the user. Both interruptions were recovered without losing the accepted models. System lock settings were not changed.

Short automation clicks did not reliably change the Bevy UI; held 200 ms clicks worked. Automated modifier shortcuts, paste, and accessibility setValue did not work as expected in egui fields. Plain End/Backspace/type/Tab editing worked; native macOS dialog setValue worked. These are automation observations and are not attributed to ordinary human input without further evidence. Tool-call latency is not an application-performance measurement.
