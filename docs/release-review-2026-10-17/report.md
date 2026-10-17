# Release usability review — October 17, 2026

**Hold the release.** The review session is complete. Five projects, drawings and
exports are saved for review. One-action Publish generated and pushed the
pedestal source/PDF/DXF/BOM/STEP to a local review repository.

| Saved project | What to review |
| --- | --- |
| [Bearing pedestal](/Users/tyler/tcad/release-review-2026-10-17/models/bearing-pedestal.tcad/project.json) | Constrained sketch, merged boss, bore, four counterbores, fillet, chamfer and drawing |
| [Sheet guard](/Users/tyler/tcad/release-review-2026-10-17/models/sheet-guard.tcad/project.json) | Formed sheet, two flanges and open hem; flat DXF exported |
| [Tube frame](/Users/tyler/tcad/release-review-2026-10-17/models/tube-frame.tcad/project.json) | Four mitered library tubes, saved steel material; no accepted weld |
| [Review assembly](/Users/tyler/tcad/release-review-2026-10-17/models/roller-fixture.tcad/project.json) | Linked parts, pedestal pattern, planar mate, BOM, saved inspection and accepted source update; partly free |
| [STEP round trip](/Users/tyler/tcad/release-review-2026-10-17/models/pedestal-step-roundtrip.tcad/project.json) | Pinned imported pedestal and saved 45° lighting rotation |

The review found and rechecked fixes for first-save location, first-face
selection, four mounting holes, Edit menu size, drawing title overflow, PDF
dispatch, ordinary frame-edge selection and theme dialog bounds. Weld support
picking now works. Each result below names the actual inputs and its limits;
these are not blanket feature-family passes.

| Active P1 hold | Findings |
| --- | --- |
| Drawing/BOM controls remain slow | RR-13, RR-29 |
| Identical Part1 inspection labels hide which components are involved | RR-11 |
| Supported miter/Butt weld remains invalid | RR-26 |
| Both pedestal/frame contacts lack a native interference result | RR-30 |
| Production drawings lack hidden-line modes | RR-18 |

Annotation placement and other selection/name/layout details also need polish.
The assembly is a review layout, not a finished engineering design. Hosted Git,
retry/conflict paths, README preview, broader feature variants and independent
export validation remain unexercised.

The user requested hands-on review and initially asked for documentation footage,
then directed the reviewer to stop recording and focus on the test drive.
Recording stopped. The main reviewer operated TCAD; this report records that
reviewer's observations and separate read-only artifact/source inspections.
No automated code tests were run. Planned coverage is in [coverage.md](coverage.md),
and the status of the earlier raw capture is in [recording-index.md](recording-index.md).

## Recommended release order

1. **Make the UI responsive.** Drawing and BOM controls still take too long to
   use. Remove repeated work during field/row rendering, then recheck the same
   saved projects. RR-13, RR-20, RR-29.
2. **Make scope names useful.** Every inspection pair currently says Part1 /
   Part1. Use a clear component path so results and update actions can be tied to
   the visible model. RR-11, RR-31.
3. **Complete the demonstrated native cases.** The supported frame Butt weld
   remains invalid, and both pedestal/frame contacts lack a native interference
   result. Use the saved models/references to reproduce these bounded failures.
   RR-26, RR-30.
4. **Finish production drawing clarity.** Add visible/hidden line modes and
   improve initial scale and dimension placement. The current PDF is readable
   only with its wireframe and overlap limitations understood. RR-18, RR-14,
   RR-16.
5. **Close the remaining release coverage.** Hosted authentication, Publish
   retry/conflict safety, README preview, texture/HDR import, wider sketch/mate
   cases, physical trackpad input and independent export validation were not
   completed. The checklist distinguishes these gaps from observed failures.

These finding groups received bounded runtime corrections in this session:
RR-01, RR-04, RR-05, RR-06, RR-08, RR-15, RR-17, RR-22, RR-23 and RR-28.
This does not close their unexercised variants or the remaining performance work.

## Environment and evidence scope

| Item | Recorded value |
| --- | --- |
| Packaged source baseline | `c6b7343bb938db64c285ce691286dd5ed327bf73` |
| Initial recheck package | `b1a6b12` and `519139e` were rebuilt and signature-verified before their rechecks. |
| Drawing recheck package | Title layout correction `1279e6c` and drawing-control/PDF correction `6114c47` were built, signature-verified and pushed. Runtime export and reopen observations below use that newer package. |
| Weld recheck package | Support-action/focus correction `cf3a078` pushed and packaged; after selection-stall correction, support picking entered Faces, accepted faces 24 and 14 and restored Pick All. Native bead remained invalid for the attempted joint. |
| Selection/theme recheck package | `633fa2f` ordinary miter-edge selection returned a menu; `f1b3fcf` Nocturne → Classic dialog bounds and OK were accepted at runtime. Broader selection chains and small-window theme layout remain unexercised. |
| Later package | `1125e2a` contains corrections for the Butt/miter shortcut clearing supports and repeated full semantic rebuilding per field. Final `4e95e72` package retained both supports through the shortcut; fields improved but remained sluggish. |
| Lighting recheck package | `4e95e72` packaged; built-in studio rotation 0 → 45 validated, applied and saved on the STEP round-trip project. |
| Final source state | Nine fix commits listed above were pushed to private origin on `codex/modeling-workflow-reliability`; final UI package `4e95e72`. Documentation commit follows this review. |
| Platform | macOS 26.5.1 (25F80), read from host system information; display scale not recorded |
| Starting theme | Classic |
| Starting content | Theme picker offered Classic only; optional theme/profile packs were not installed at first inspection |
| Review artifacts | `/Users/tyler/tcad/release-review-2026-10-17/` |
| Interaction | Native desktop UI through computer-control tooling; actual human trackpad comfort not established |
| Video | Recorder check reported clean; first task capture failed with no movie file. Videos were then deferred by the user. |

The opening empty Part1 showed a valid evaluation labeled 2 ms. This is one
visible evaluation value, not an app launch-time or performance benchmark.
Computer-control round-trip duration is also not an application benchmark.

## Build and validation record

Commit messages and the main reviewer's package records document these checks:

| Check | Actual scope |
| --- | --- |
| Build | Workspace builds throughout; final `4e95e72` explicitly records a locked workspace build |
| Formatting | Workspace formatting checks; final `4e95e72` also records a whitespace/diff check |
| Strict lint | `cf3a078`: locked workspace, all targets, warnings treated as errors. Other commits used the affected core, kernel or desktop crates. Final `4e95e72` lint was **cad-desktop only**, not a new whole-workspace lint |
| macOS package | Packaged app rebuilt and signature-verified for the reported UI rechecks; final UI build `4e95e72` |
| Automated tests | None added or run |

Compilation/lint/package success is separate from the interactive results below.
No Windows/Linux acceptance is claimed.

## Observed coverage

Rows below name only what was observed. Remaining checklist modes are Not run.

| Checklist | Status | Observed result | Evidence / remaining work |
| --- | --- | --- | --- |
| R01 — Launch and reopen | Partial; named reopens passed | Baseline and rebuilt packages opened empty Part1. The fillet checkpoint reopened with valid 196 ms. The complete pedestal reopened with valid 576 ms; tube frame with valid 110 ms. Review assembly reopened with its four active occurrences and valid 1,735 ms. | Guard standalone reopen Not run; later frame selection stalled (RR-22). No cross-version/platform claim. |
| R02 — Empty start and fallback | Partial | Empty Part1 was shown. Classic was the only initial theme choice, with no optional theme/profile pack installed. | Broader no-pack modeling/fallback checks Not run. |
| R07 — Grouped task controls | Partial | First-sketch plane chooser and visible groups/dividers were present; sketch/feature/assembly/drawing tasks were subsequently exercised. | Systematic grouping review across tasks and narrow-rail layout checks Not run. |
| R03 — Navigation | Partial | Isometric view and F framing showed a clean 120 × 80 × 12 mm plate with visible black edges. | Full orbit, poles, physical gestures and remaining navigation modes not yet checked. |
| R09 — Theme switching | Partial; persistence and corrected reverse switch passed | Classic → Nocturne preview changed layout/style. Later OK saved Nocturne, and restart retained it. Nocturne → Classic initially overflowed dialog bounds; Escape recovered. Rebuilt `f1b3fcf` kept title/footer/actions visible, and OK saved Classic. | Smaller windows, additional densities and active-modeling switches remain unexercised. See RR-23. |
| R10 — Settings Cancel | Partial | Cancel restored Classic and displayed “Unapplied settings discarded”. Later Nocturne Save/restart and corrected Classic Save succeeded. | Broader density/size changes and model-scope preferences Not run. |
| L01 — Missing-library route | Partial | Theme/profile download routes were available; the screen visibly warned that source packs are private. | Missing permissions, offline behavior and modeling-draft preservation not exercised. |
| L02 — Install packs | Partial | Theme, weldment-profile and texture downloads visibly completed and showed Installed status. Material download was started. Switching back to Themes showed “Libraries refreshed”. Library profile insertion later succeeded for the frame. | Material installation has not yet been visually confirmed; refresh is not treated as install success. Texture assignment and hardware pack remain unexercised. |
| K01/K02 — First planar sketch | Partial | A Top-plane rectangle was drawn at the origin in the model viewport. Automatic horizontal/vertical/coincidence relations left two reported degrees of freedom. | Only this rectangle/support case exercised; other geometry and placement modes not yet checked. |
| K03 — Smart Dimension | Partial | Width 120 mm reduced reported DOF to 1; height 80 mm reduced it to 0. | Tool-first circle-center/edge and Cmd-click preselection paths not yet exercised. |
| K06 — Live dimension/Cancel | Partial | Editing height 80 → 90 mm visibly moved the top edge, with the picked right edge orange. Cancel Dimension restored the 80 mm geometry and the transaction's 1-DOF state. Recreating height 80 mm returned to 0 DOF. | Current dimension preview and local cancellation observed; handle dragging/fixed geometry/reference modes not yet checked. |
| M01 — Extrusion and face-attached join | Partial | Finish Sketch committed the 12 mm base. A face-attached Sketch2 with radius 20 mm drove a 25 mm boss with Merge Result. The boss showed valid 5 ms evaluation and one solid body. The completed downstream chain saved and reopened. | Preview difference comparison and parameter-change propagation not yet checked. |
| M04 — Bore and mounting holes | Partial; recheck passed for named inputs | A Ø20 Through All bore applied. Four mounting positions initially failed on the pre-fillet stock. After rebuild, the same positions recreated after Fillet1 gave a valid Ø5 Blind 10 mm preview and valid Through All counterbores Ø10 × 3 mm; the counterbores applied with valid 23 ms and one body, saved and reopened. | Original exact pre-fillet target and standalone simple Through All recheck not performed. See RR-06. |
| M06 — Fillet and adjoining chamfer | Partial; named cases passed | One outer-boss-rim click selected four segments; radius 3 mm preview was localized, then applied and saved. After rebuild, the fillet reopened rounded. Two adjoining 120/80 mm straight top-plate edges then accepted a 2 mm chamfer with valid 23 ms; the complete chain saved and reopened. | Other connected/curved edge treatments remain unexercised; incompatible pick-filter handoff remains RR-07. |
| M05 — Downstream counterbore regression | Baseline failed; revised-chain recheck passed | Native ancestry error blocked four holes on plate/boss/bore. After the fix, those positions accepted Blind preview and counterbores on the later Fillet1 stock. | Exact original pre-fillet target not repeated. Pockets and existing-bore counterbore variant have not yet run. |
| R04 — Face selection/highlight | Baseline failed; recheck passed | Baseline Pick Faces required a second click for face/context. Rebuilt Pick All retained the base face and context on the first click. | Specific Pick All behavior accepted; exact Pick Faces repetition and edge/component/editor-scope follow-through still need checks. See RR-05. |
| X01 — Failure cancellation | Partial | Cancel after the four-hole error restored the pre-operation part and removed temporary Sketch4; saving that accepted state succeeded. | Rapid-change stale-result and other cancellation cases remain unexercised. |
| R13 — First Save | Partial; corrected suggestion passed | Baseline first Save proposed filesystem root; manually choosing review folder saved successfully. Rebuilt empty Part1 proposed `~/TCAD/Projects/Part1.tcad/project.json`; Cancel did not create a file. | Save As copy isolation, rejected-path handling and other defaults remain unexercised. See RR-04. |
| R08 — Compact Edit menu | Partial; corrected popup passed | Rebuilt Edit menu measured about 94 logical pixels instead of about 600 for its four items. | Other menus, keyboard focus and themes remain broader checks. See RR-01. |
| D01 — First drawing sheet | Partial; sheet saved and reopened | Baseline title overflow prevented paper. Rebuild rendered A4/front/top/right/full title. After performance fixes, manual scale/dimensions applied and saved; reopening Drawing retained Sheet, 1:2 scale and both dimensions. | Calls improved to about 15 seconds from 35–50 seconds; responsive interaction remains unaccepted. See RR-08/RR-13. |
| D02 — View scale | Partial; manual scale passed | Default 1:5 made the 120 × 80 mm part tiny. Manual 1:2 worked. | Default fit recommendation remains; broader alignment and direct paper interaction not completed. See RR-14. |
| D03 — Drawing dimensions | Partial | Smart Dimension on top-view rectangle corners created 120 mm and 80 mm dimensions; Apply, Save and reopen preserved them. | PDF confirms height still inside part and width extension crossing caption; attempted height drag did not move it (RR-16). Circular diameter not yet achieved. |
| D09 — PDF/DXF drawing export | PDF route corrected; named outputs created | Initial PDF action reported unknown command. New package opened the PDF dialog and wrote a 982,897-byte PDF; rendered page was inspected. Drawing DXF also exported (141,869 bytes). | Page fits/title wraps; annotation overlaps remain. External drawing-DXF import Not run. Internal edges are solid with no hidden-line mode (RR-18). |
| G14 — STEP export/reimport | Partial; visual round trip and Save accepted | UI wrote `bearing-pedestal.step`, 270,857 bytes. Import preview/Confirm Changes produced a recognizable complete body, valid 72 ms. Selecting imported part showed Solid Bodies(1), Pinned STEP solids and cleared the first-sketch overlay. Separate round-trip project saved. | No precision, units, imported-project reopen or independent-viewer comparison yet. Initial active-scope handoff remains RR-32. |
| F01 — Sheet base | Partial; named base passed | Roller Fixture - Sheet Guard: 700 × 120 mm Top-plane rectangle was fully constrained with two Smart Dimensions. Base Flange thickness 2 mm, radius 2 mm and K factor 0.4 applied with valid 3 ms. | Other thickness modes and saved reopen not yet exercised. |
| F02 — Flanges and hem | Partial; named chain passed | Two opposing long-edge flanges 25 mm at 90° applied with valid 14/22 ms. Open Hem length 10 mm, gap 1 mm attached to Edge Flange2 through an on-model terminal pick and applied with valid 42 ms. | Base-edge preselection was not carried into the new flange; explicit pick worked (RR-09). Jog and other modes not yet exercised. |
| F04 — Sheet state and export | Partial; artifact content inspected | Flat state applied with valid 11 ms; Formed restoration with valid 73 ms, then saved. UI export succeeded. Read-only DXF inspection found a closed connected 700 × 192.880529879223 mm outline and three 700 mm bend lines. | External CAD/CAM viewer import and saved reopen Not run; content inspection is not complete manufacturing acceptance. |
| F05 — Library structural members | Partial; named group and reopen passed | A fully constrained 700 × 400 mm Top-plane rectangle supplied all four paths. Library square tube 40 × 40 × 3 mm, graphical Top center location, Miter and zero gap produced four bodies, valid 46 ms, then saved. Clean rebuilt app reopened it with valid 110 ms. | Mixed groups and trim/cap/gusset remain unexercised. Later seam click stalled selection UI (RR-22). |
| F08/F09 — Visible weld and recovery | Pickers and shortcut corrected; supported bead still failed | Rebuilt controls entered temporary Faces, accepted faces 24 and 14 and restored Pick All. Final recheck on edge 63 retained both supports through Use butt/miter and followed joint faces. Butt width 3 mm / crown 0.5 mm still failed native construction in earlier trim variants; final zero-trim toe diagnostic remained. Cancel restored valid 105 ms frame. | No accepted bead. RR-17 UI handoff/retention is corrected; bounded native supported-miter failure remains RR-26. |
| F10 — Frame cut-list export | Partial; source/output values agree | File menu export succeeded. Read-only 853-byte CSV has four source-member rows: two at 400 mm and two at 700 mm, quantity 1 each, both end angles 45°, nominal square tube 40 × 40 × 3 mm, Aluminum. | Matches saved four-member path/miter/material intent. Exact saw-blank convention, external CSV import and drawing cut-list not validated; headers need units/reference clarity (RR-19). |
| A01 — Insert existing projects | Partial; inserts, Save and reopen accepted | Frame, pedestal and guard were inserted through Insert Existing as linked project-root subassemblies; all three validated (113 ms). Saved review assembly reopened with four active occurrences after the pedestal pattern (1,735 ms). A later controlled frame update applied and saved (A15). | All initial component labels/reference choices read Part1 (RR-11). Other nesting/uniqueness/edit-in-place variants not checked. |
| A02 — Component placement | Partial | First frame was grounded. Pedestal translated to (60,240,0) mm. Guard translated to (0,-30,120) mm with X rotation 90°. | Numeric placement accepted; handle dragging, cohort movement, Cancel/Undo and precise engineering fit not checked. |
| A03 — Basic mate | Partial; one planar case passed | Coincident mate between frame/pedestal Top planes accepted with zero gap/residual, 3 independent equations and 3 remaining DOF. | Remaining motion is expected and explicitly retained; assembly is not fully constrained. Other mates unexercised. |
| A01 — Occurrence naming | Partial; rename applied and saved | Expanded Components row exposes an occurrence-name field. Bearing Pedestal rename applied and saved; source inspection confirms the saved name, and the assembly reopened. A name-only edit triggered full evaluation labeled 3,449 ms / No material change. | Definition/root labels remain Part1; all visible reopened label states not separately recorded. See RR-20. |
| A06 — Component pattern | Partial; named linear pattern passed | Component Pattern1 uses Bearing Pedestal seed, reversed linear X, count 2 including seed and 700 mm spacing. New-copy preview was green and in the intended position. Applied, valid 113 ms, saved. | Picked line/axis/edge direction, second direction and count editing not yet checked. Large direction arrow obscured model area (RR-21). |
| G08 — One-action Publish | Partial; local destination passed | File → Publish Settings saved local bare-repository URL, codex/release-review branch, model folder and PDF/DXF/BOM/STEP choices. Publish UI confirmed uploaded commit `01f23804b745`; read-only remote inspection confirms that branch at the full commit. | No external server used. Retry, conflicts, unrelated remote files and credential flows remain unexercised. See artifact inspection below. |
| G09 — Remembered Publish choices | Partial; settings file verified | Source `tcad-publish.json` contains the exact local URL, branch, model folder and four selected outputs; other outputs are false. | Reopening the dialog/project and republishing after changes not yet checked. Nocturne layout requires much scrolling and has label overflow (RR-25). |
| G10 — Publish saved drawing | Partial; saved-sheet route passed | Publication manifest reports one saved drawing sheet, revision 10, PDF/DXF/BOM/STEP and no README preview. Published PDF/DXF match the earlier UI exports byte for byte; STEP differs only by its header timestamp. | Automatic projections for projects without saved sheets and multi-sheet publication remain unexercised. |
| R04 — Frame edge selection | Initial stall; ordinary-edge recheck passed | Original miter-seam click stalled. With `633fa2f`, an ordinary frame miter-edge click returned its context menu in 9 seconds. | Explicit chain expansion not rechecked; 9 seconds is an end-to-end computer-control duration. See RR-22. |
| L06 — Material and independent finish | Material applied and saved; finish preview only | Search S355 → Structural steel S355J2H → Assign material → Apply updated the tree to steel and left the optical finish unchanged. Steel frame then saved. RAL1015 Light Ivory Matte produced a valid cream preview. | Finish not applied; lighting blocked the draft. Face/occurrence scopes and mass not checked. Assembly kept Aluminum until its explicit linked update, then adopted steel. |
| L08 — Lighting | Corrected built-in rotation applied and saved | Original 0 → 45 draft blocked Apply. With `4e95e72` on the imported project, built-in studio rotation 0 → 45 showed Valid proposed geometry, enabled Apply and accepted one reported undoable change. File Save succeeded; saved source records 45°. | Original simultaneous RAL finish draft not repeated. HDR/texture import and actual Undo/reopen not exercised. See RR-28. |
| A17 — Assembly BOM/cut list | Quantities and CSV passed; UI slow | Reopened assembly evaluated one guard, two pedestals, one frame and four cut-list members at 700, 700, 400, 400 mm. CSV confirms three rows with quantities 1/2/1 and the then-pinned Aluminum material. | Export predates the explicit steel source update. BOM panel had long timeouts/returns (RR-29); drawing tables and post-update CSV not exercised. |
| A12 — Interference/clearance | Inspection applied and saved; partial native result | Interference and approximate boundary, 1 mm clearance, 0.05 mm resolution, four targets: six pairs reported. Four were separated at approximate distances 190, 10, 190, 580 mm; two pedestal/frame contact pairs had zero approximate distance and unavailable native check. Inspection applied/saved, valid 3,136 ms. | All six pair labels read Part1 / Part1, including renamed pedestal. No clean bill of clearance/interference; see RR-11/RR-30 and pair map. |
| A15 — Controlled linked update | Named material update passed | Assembly Library Check found frame source revision 4 versus pin 3. Preview validated and reported impact: two component occurrences, one mate, zero drawings, one changed BOM row. Apply accepted one update, valid 3,122 ms; Save succeeded. | Read-only saved source confirms imported frame steel and pin 4. Cancel/Undo execution, geometry-changing update and other receiving assemblies not checked. UI association/change wording difficult (RR-31). |

Screenshot: [Classic theme settings](/Users/tyler/tcad/release-review-2026-10-17/evidence/01-classic-theme-settings.png).
Screenshot: [Nocturne theme preview](/Users/tyler/tcad/release-review-2026-10-17/evidence/02-nocturne-preview.png).
Screenshot: [Smart Dimension live preview](/Users/tyler/tcad/release-review-2026-10-17/evidence/03-smart-dimension-live-preview.png).
Screenshot: [Four-counterbore budget error](/Users/tyler/tcad/release-review-2026-10-17/evidence/04-four-counterbores-budget-error.png).
Screenshot: [Pedestal with counterbores, fillet and chamfer](/Users/tyler/tcad/release-review-2026-10-17/evidence/05-pedestal-counterbores-fillet-chamfer.png).
Screenshot: [Formed sheet guard](/Users/tyler/tcad/release-review-2026-10-17/evidence/07-sheet-guard-formed.png).
Screenshot: [Initial drawing title failure](/Users/tyler/tcad/release-review-2026-10-17/evidence/06-default-drawing-title-overflow.png).
Screenshot: [Pedestal drawing workspace](/Users/tyler/tcad/release-review-2026-10-17/evidence/08-pedestal-drawing.png).
Rendered PDF: [Exported drawing page](/Users/tyler/tcad/release-review-2026-10-17/evidence/09-exported-drawing.png).
Screenshot: [Frame weld attempt and error](/Users/tyler/tcad/release-review-2026-10-17/evidence/10-frame-weld-error.png).
Screenshot: [Review assembly with pedestal pattern](/Users/tyler/tcad/release-review-2026-10-17/evidence/11-review-assembly.png).
Screenshot: [Classic Settings dialog bounds corrected](/Users/tyler/tcad/release-review-2026-10-17/evidence/14-classic-dialog-bounds-fixed.png).
Screenshot: [Supported miter weld failure](/Users/tyler/tcad/release-review-2026-10-17/evidence/15-supported-miter-weld-failure.png).
Screenshot: [Lighting preview remains blocked](/Users/tyler/tcad/release-review-2026-10-17/evidence/16-lighting-preview-stalled.png).
Screenshot: [Weld supports retained after correction](/Users/tyler/tcad/release-review-2026-10-17/evidence/21-weld-supports-retained.png).

## Current review models

### Bearing pedestal — saved

The working model is named **Roller Fixture - Bearing Pedestal**. It currently
contains a 120 × 80 mm constrained Top-plane rectangle, a 12 mm base extrusion,
a face-attached Ø40 × 25 mm merged boss, a centered Ø20 through bore, a 3 mm
fillet around the complete outer boss top rim, four Ø5 Through All mounting holes
with Ø10 × 3 mm counterbores, and a 2 mm chamfer on two adjoining straight
top-plate edges. Initial failed hole drafts were canceled; after rebuild, the
new accepted holes target Fillet1 and are in the saved checkpoint. The save path is
`/Users/tyler/tcad/release-review-2026-10-17/models/bearing-pedestal.tcad/project.json`.
The UI confirmed **Saved: bearing-pedestal.tcad** and the full path. A read-only
filesystem check also found its manifest and part/assembly files. The
[saved pedestal project](/Users/tyler/tcad/release-review-2026-10-17/models/bearing-pedestal.tcad/project.json)
is available. Both the Fillet1 checkpoint and the complete later
counterbore/chamfer checkpoint reopened successfully. Displayed
evaluation times throughout this report are individual observations, not broad
performance measurements.

A drawing sheet with manual 1:2 scale and 120/80 mm dimensions applied and saved
in the same project. Reopening Drawing retained the sheet, scale and dimensions.
After the PDF command correction, the UI exported these files:

| Output | File | Observed verification |
| --- | --- | --- |
| Drawing PDF | [bearing-pedestal-drawing.pdf](/Users/tyler/tcad/release-review-2026-10-17/exports/bearing-pedestal-drawing.pdf) | 982,897 bytes; rendered page inspected, fits paper and wraps title; annotation overlaps remain |
| Drawing DXF | [bearing-pedestal-drawing.dxf](/Users/tyler/tcad/release-review-2026-10-17/exports/bearing-pedestal-drawing.dxf) | 141,869 bytes; UI saved file, external import not checked |
| STEP | [bearing-pedestal.step](/Users/tyler/tcad/release-review-2026-10-17/exports/bearing-pedestal.step) | 270,857 bytes; later UI reimport visually recognizable and valid 72 ms, precision/units comparison not checked |

In the rendered PDF, front/right internal edges appeared as continuous solid
lines. Source audit confirmed there is no hidden-line control to change this.
The coverage inventory already describes standard views as wireframe and exact
hidden-line removal as a gap. This is a P1 production-output limitation observed
in the actual drawing, not a misapplied setting; see RR-18.

### Sheet guard — saved formed model, flat DXF exported

The working **Roller Fixture - Sheet Guard** contains a fully constrained
700 × 120 mm Top-plane rectangle, a 2 mm Base Flange with radius 2 mm and K factor
0.4, two opposing 25 mm/90° long-edge flanges, and an Open Hem of length 10 mm with
gap 1 mm attached to Edge Flange2. Flat and then Formed fabrication states
applied. The formed model saved at
[sheet-guard.tcad/project.json](/Users/tyler/tcad/release-review-2026-10-17/models/sheet-guard.tcad/project.json).
The UI reported successful export of
[sheet-guard-flat.dxf](/Users/tyler/tcad/release-review-2026-10-17/exports/sheet-guard-flat.dxf).

Read-only artifact inspection found:

- DXF: 3,300 bytes, AC1027 format, millimeter units; closed connected outline
  represented by ten degree-1 SPLINE entities, with three 700 mm bend LINEs.
- Extents: 700 × 192.880529879223 mm. The artifact reviewer reported agreement
  with the developed width expected from the recorded thickness, radii, K factor
  and hem settings. No geometry discrepancy was identified by this inspection.
- Saved guard: three JSON source files totaling 9,712 bytes, resolved references,
  complete feature chain and formed state. No logs or mesh caches in that saved
  project.

External CAD/CAM viewer import and standalone saved-model reopen were Not run.
This is source/output inspection, not a claim of production-machine validation.

### Tube frame — saved without a weld bead

**Roller Fixture - Tube Frame** uses a fully constrained 700 × 400 mm Top-plane
rectangle. Structural Member1 consumes all four paths with a library
40 × 40 × 3 mm square tube, graphical **Top center** locating point, **Miter**
corners and zero gap. The accepted group has four bodies and reported valid
46 ms. It saved at
[tube-frame.tcad/project.json](/Users/tyler/tcad/release-review-2026-10-17/models/tube-frame.tcad/project.json).

The visible weld attempt failed and was canceled. The saved frame has no weld
bead; its stock remained intact. The saved project was subsequently inserted
into the review assembly and grounded. Clean rebuilt `cf3a078` app reopened the
standalone frame with valid 110 ms. Clicking its front miter seam in Pick All
then stalled the UI; the process was terminated for recovery before weld support
controls could be rechecked (RR-22). Mixed sections remain unexercised.

After selection and support-control fixes, picking top support faces 24 and 14 worked
and restored Pick All. A Butt bead with width 3 mm/crown 0.5 mm remained invalid
with zero, one-end 3 mm or both-end 3 mm trim. The final diagnostic was an
unsupported prismatic arrangement with near-parallel lines neither certified
coincident nor separated. Apply stayed disabled; Cancel restored the valid
107 ms frame. The failed bead was not saved (RR-26).

Final `4e95e72` recheck reopened the frame with valid 107 ms. Selecting seam edge 63,
picking supports 24 and 14 and choosing **Use butt/miter** retained both exact supports;
the Butt profile/orientation followed the joint faces. The zero-trim outside-toe
diagnostic remained. Cancel restored the saved frame with valid 105 ms. No valid
bead was created; the corrected support workflow is distinct from RR-26.

In the subsequent appearance review, assigning **Structural steel S355J2H**
applied and changed the tree material while leaving the optical finish unchanged.
This material change subsequently saved, and read-only source inspection confirms
**Structural steel S355J2H**. A Part Definition preview of **RAL 1015 Light Ivory Matte** made the
frame cream-colored, but was not applied. Changing lighting rotation 0 → 45
then blocked Apply; that finish was not committed (RR-28). Earlier cut-list/source
inspection below refers to the saved Aluminum checkpoint.

File-menu export produced
[tube-frame-cutlist.csv](/Users/tyler/tcad/release-review-2026-10-17/exports/tube-frame-cutlist.csv)
(853 bytes). Read-only comparison with the saved source found:

| Member rows | Reported length | Start / finish angle | Quantity | Section / material |
| --- | --- | --- | --- | --- |
| 1 and 2 | 400.000 each | 45.000° / 45.000° | 1 each | Nominal square tube 40 × 40 × 3 mm / Aluminum |
| 3 and 4 | 700.000 each | 45.000° / 45.000° | 1 each | Same |

These values match the two 400 mm and two 700 mm saved path members, four explicit
member selections, Miter, zero start/end offset, zero joint gap, the pinned nominal
profile and the part's default Aluminum material. No missing/duplicate row or
numeric inconsistency was found in that comparison. This establishes agreement
with saved intent, not an independent measurement of long-point saw blanks.
The CSV's generic numeric headers omit units and the length convention (RR-19).

### Fixture assembly — saved review layout

Insert Existing brought in the three saved project roots as linked subassemblies.
The frame was grounded first. The pedestal was placed at **(60, 240, 0) mm**, and
the guard at **(0, -30, 120) mm** with **X rotation 90°**. This places the pedestal
on the near short frame rail and the guard upright outside a long side. These
are review-layout positions, not an engineered mounting fit.

A coincident mate between the frame and pedestal Top planes accepted with zero
gap and residual, three independent equations and three remaining degrees of
freedom. All three inserted projects validated with the displayed 113 ms status.
The assembly remains partly free. It saved successfully as
**Roller Fixture - Review Assembly** at
[roller-fixture.tcad/project.json](/Users/tyler/tcad/release-review-2026-10-17/models/roller-fixture.tcad/project.json).
The saved assembly later reopened with four active occurrences (two pedestals,
guard and frame) and valid 1,735 ms. All three initial component/reference labels displayed Part1.
An occurrence-name field is available by expanding a Components row. Renaming
the pedestal occurrence **Bearing Pedestal** applied, and the assembly saved
again. Definition/root labels remain Part1.

**Component Pattern1** then used Bearing Pedestal as its seed: linear X reversed,
count 2 including the seed, spacing 700 mm. The green preview showed the new
copy at the intended far frame rail. The pattern applied with valid 113 ms and
saved. Its oversized direction arrow obscured model area (RR-21).

Read-only inspection of this initial saved checkpoint found 11 files totaling
207,164 bytes: the manifest, three parts, four assemblies and three component
snapshot resources. All manifest part/assembly/resource paths resolved. The
root file records the three stated placements, fixed frame and one coincident
Top-plane mate. Linked-source entries retain source paths, revisions, hashes and
pinned definitions. The initial inspection did not establish update/portability;
the later explicit material-update workflow is recorded below.

The reopened assembly's BOM showed **1 guard, 2 pedestals and 1 frame**, with
four frame cut-list rows at **700, 700, 400, 400 mm**. File export produced
[roller-fixture-bom.csv](/Users/tyler/tcad/release-review-2026-10-17/exports/roller-fixture-bom.csv)
(303 bytes). Read-only inspection confirms the three named BOM rows, quantities
1/2/1 and Aluminum. At that point the frame retained its imported Aluminum pin;
the separately saved steel source had not yet been explicitly updated into this
assembly. That was expected controlled-update behavior. The CSV remains the
historical pre-update output after the later accepted change below.
The [BOM inspection record](/Users/tyler/tcad/release-review-2026-10-17/evidence/17-assembly-bom-inspection.json)
preserves the source comparison. BOM controls remained slow (RR-29).

**Interference and Clearance1** applied and saved with four targets,
Interference plus approximate boundary mode, 1 mm requested clearance and
0.05 mm resolution. It reported six of six pairs with valid 3,136 ms:

| Pair, resolved from saved references | Approximate boundary distance | Reported native result |
| --- | --- | --- |
| Guard / near pedestal | 190 mm | Separated |
| Guard / frame | 10 mm | Separated |
| Guard / far pedestal | 190 mm | Separated |
| Near pedestal / frame | 0 mm | Unavailable; parameter-domain/loop-orientation diagnostic |
| Near pedestal / far pedestal | 580 mm | Separated |
| Frame / far pedestal | 0 mm | Unavailable; same diagnostic class |

The UI labeled every pair **Part1 / Part1**. Read-only resolution of saved target
paths and evaluator pair order identifies the two unavailable results as the
expected pedestal/frame contacts. This mapping is not a new geometry evaluation
or a clearance certificate. The definitions are saved for reproduction;
[pair map](/Users/tyler/tcad/release-review-2026-10-17/evidence/18-assembly-inspection-pair-map.json)
records the references and parent-observed values. The review does not claim
the assembly is interference-free (RR-30).

**Controlled frame update:** Assembly Library **Check** found source revision 4
against pinned revision 3. **Preview update** validated and reported two affected
component occurrences, one mate, zero drawings and one changed BOM row.
**Apply** accepted the update with valid 3,122 ms, then Save succeeded. Read-only
saved inspection confirms document revision 9, linked frame source revision 4 and
the imported part's **Structural steel S355J2H** material. The
[update evidence](/Users/tyler/tcad/release-review-2026-10-17/evidence/19-linked-frame-update-inspection.json)
records the link and resolved material. This establishes an explicit accepted
material update; Cancel/Undo execution, geometry-changing updates and propagation
to another assembly remain unexercised.

### Local Publish — pedestal source and outputs committed and pushed

In Nocturne, **File → Publish Settings** was configured with:

- Repository: `/Users/tyler/tcad/release-review-2026-10-17/remotes/review.git`,
  a prepared empty local bare repository.
- Branch: `codex/release-review`; project folder: `model`.
- Enabled: drawing PDF, drawing DXF, BOM CSV and STEP. Other outputs were off.
- Local review author: `TCAD Release Review <release-review@tcad.invalid>`.

Save choices succeeded. Publish reported uploaded commit **01f23804b745**.
Independent read-only Git inspection confirmed
`refs/heads/codex/release-review` at
`01f23804b745714b4103b2859ed0c883a9436c46`. No external forge/server was used.

The committed `model/` package has ten files totaling 1,475,215 bytes: source
manifest/part/assembly, saved Publish choices, publication metadata and managed
file inventory, plus four outputs. Every recorded output and managed-file SHA-256
matched its actual file. The 143-byte BOM contains one row: **Roller Fixture -
Bearing Pedestal**, quantity **1**, material **Aluminum**, which agrees with the
standalone source. This does not exercise nested assembly quantities.

Published PDF (982,897 bytes) and DXF (141,869 bytes) are byte-identical to the
earlier UI exports. Published STEP (270,857 bytes) differs only in its FILE_NAME
timestamp. The publication manifest identifies revision 10, one saved sheet and
`readme_preview: false`. The earlier STEP subsequently reimported visually into
TCAD; independent-reader verification and README preview remain untested.

The package is retained outside Git at
[published-pedestal-01f23804b745](/Users/tyler/tcad/release-review-2026-10-17/evidence/published-pedestal-01f23804b745).
The detailed [artifact inspection](/Users/tyler/tcad/release-review-2026-10-17/evidence/13-publish-artifact-inspection.json)
records inventory, hashes and BOM values. The original publication job used a
separate checkout under `~/.tcad/publishing/jobs/`; it did not publish from the
application source repository.

### Pedestal STEP round trip — imported and saved separately

Importing the earlier exported `bearing-pedestal.step` into a new document
produced a preview; **Confirm Changes** accepted it with Geometry valid 72 ms and
a recognizable complete body. This is visual acceptance of the named round
trip, not a precision, topology, units or parametric-history equivalence claim.

The active part remained empty Part1. The tree showed **No features yet** and the
viewport placed **Start your first sketch** over the imported body. The part
selector offered Assembly, Part1 and Roller Fixture - Bearing Pedestal. Selecting
the imported part showed **Solid Bodies(1)** and **Pinned STEP solids**, and the
empty-sketch overlay disappeared. The project saved as **Roller Fixture - STEP
Round Trip** at
[pedestal-step-roundtrip.tcad/project.json](/Users/tyler/tcad/release-review-2026-10-17/models/pedestal-step-roundtrip.tcad/project.json).
That is a successful workaround; the initial scope handoff remains RR-32.

On this project, lighting correction `4e95e72` allowed built-in studio environment
rotation **0 → 45**: Valid proposed geometry, enabled Apply, one reported undoable
change and successful File Save. Read-only source inspection confirms
`environment_rotation_deg: 45.0`. The saved project includes its pinned STEP
resource. Imported-project reopen and independent precision/units comparison
remain unexercised.

## Findings

| Finding | Current state |
| --- | --- |
| RR-01 Edit popup size | Corrected; compact four-item popup observed in rebuilt app |
| RR-02 Library-list movement | Open usability follow-up |
| RR-03 Private starter packs | Release distribution decision, not a download failure on this Mac |
| RR-04 First-save destination | Corrected; sensible new-project path and no-file Cancel observed |
| RR-05 First-click face selection | Corrected in Pick All recheck; remaining selection modes not all repeated |
| RR-06 Mounting-hole ancestry budget | Corrected in later Fillet1-chain recheck; original exact target and all variants not repeated |
| RR-07 Fillet selection filter | Open usability follow-up; manual Pick Edges worked |
| RR-08 Default drawing title overflow | Corrected; A4 paper, full wrapped title and enabled Apply observed in rebuild |
| RR-09 Flange does not use edge preselection | Open P2 usability observation; explicit base-edge picker worked |
| RR-10 Sheet-edge context omits sheet tools | Open P2 discoverability observation |
| RR-11 Part1 labels obscure selection and inspection scope | Open P1 for inspection interpretation; all six result pairs have identical names, despite outer occurrence rename |
| RR-12 Flat DXF interoperability/presentation | P2 artifact-inspection recommendations; no confirmed geometry discrepancy |
| RR-13 Drawing manager responsiveness | Improved in later rebuild; calls about 15 seconds versus 35–50, responsive interaction remains unaccepted |
| RR-14 Default part-drawing scale | Open P2 default-layout follow-up; manual 1:2 worked |
| RR-15 Drawing PDF action not wired | Corrected; dialog, actual PDF output and rendered page inspected |
| RR-16 Initial dimension placement overlaps drawing | Open P2; overlaps remain in exported PDF and attempted height drag did not move annotation |
| RR-17 Part weld support picking and shortcut recovery | Corrected; temporary Faces/restored All and exact support retention through Butt/miter accepted in final package |
| RR-18 No hidden-line production drawing mode | Open P1 output limitation; internal edges exported as solid and no toggle exists |
| RR-19 Cut-list numeric headings lack units/reference | Open P2 output clarity; reported values agree with saved member intent |
| RR-20 Occurrence rename rebuilds geometry | Open P2 responsiveness; a name-only result displayed 3,449 ms / No material change |
| RR-21 Component-pattern arrow obscures model | Open P2 visual polish; geometry/copy preview and accepted pattern were correct |
| RR-22 Frame seam selection stalls the UI | Corrected ordinary-edge recheck returned menu in 9 seconds; explicit chain expansion not repeated |
| RR-23 Theme switch pushes Settings actions offscreen | Corrected; Nocturne → Classic kept title/footer/actions visible, OK saved preference |
| RR-24 Theme-picker and advanced-editor entry labels | Open P2 discoverability; Layout and Theme opens large source editors while picker is under Settings |
| RR-25 Nocturne Publish panel is cumbersome | Open P2 layout polish; much scrolling and label overflow, but setup and Publish completed |
| RR-26 Supported miter/Butt bead fails native construction | Open P1 bounded geometry blocker; explicit supports accepted, Apply disabled for all attempted trim variants |
| RR-27 Material/finish editor entry and scope are unclear | Open P2 usability; material tree click did nothing visible, both editors remained open with duplicate actions |
| RR-28 Lighting rotation leaves draft invalid/evaluating | Corrected built-in studio recheck: 0 → 45 preview, Apply and Save succeeded on imported project |
| RR-29 Assembly BOM panel is very slow | Open P1 responsiveness; output quantities/export correct, but 30-second timeout and 45–52-second returns |
| RR-30 Native contact inspection unavailable | Open P1 bounded inspection gap; both pedestal/frame contacts unresolved, approximate distances explicitly not certification |
| RR-31 Linked update controls and change preview are hard to read | Open P2 usability; identical Part1 actions detached from paths and raw IDs/JSON metadata obscure a successful update |
| RR-32 STEP import leaves empty Part1 active | Open P2 scope handoff; accepted imported body shows first-sketch overlay until its part is selected |

### RR-01 — P2: Edit menu is much larger than its contents

The native Edit popup occupied approximately 600 logical pixels for four items.
This wastes space and makes a short menu look sparse. The theme review agent
identified a likely source cause; a code finding is not runtime acceptance of a
fix.

**Recheck:** after rebuilding, the Edit popup was about 94 logical pixels tall
for the same four items. This accepts the compact popup case, not all menu/theme
layouts.

Recommended change: size a short popup to its actual rows and labels, with a
reasonable maximum size for longer menus. Check keyboard focus and both themes
after any change. No modeling failure was observed from this popup.

### RR-02 — P2: Library list moves during installation

The content-library list changed order when a pack was installed. The completed
status was clear, but moving rows can make it harder to continue through setup
or locate the next pack.

Recommended change: keep starter-pack rows in stable order during transfers and
status changes. If installed and available packs are separate sections, move the
row only after a deliberate refresh or provide a clear transition. This is a
low-severity usability observation; theme, profile and texture installations
subsequently completed.

### RR-03 — Release distribution decision: starter packs require access

The UI visibly warned that the starter content repositories are private. Theme
and profile downloads succeeded on this Mac, so this is not an observed
authentication failure. A public release still needs a clear decision about who
can access these sources and how an unentitled user obtains optional content.

Recommended action: decide content-pack distribution before release, retain
usable offline/no-pack fallbacks, and keep the source URL/access requirement
visible. This review does not authorize making any repository public.

### RR-04 — P1: first Save suggests a filesystem-root project path

For the new named pedestal, File → Save opened Save As with
`/Roller Fixture - Bearing Pedestal.tcad/project.json`. This puts the default at
the filesystem root rather than an ordinary personal project location. It makes
the first save confusing and can lead to a permission failure.

The reviewer selected a path under the session's models folder and the save
succeeded with a clear status and complete path. No source loss was observed.
**Recheck:** rebuilt empty Part1 proposed
`~/TCAD/Projects/Part1.tcad/project.json`; canceling created no file. The original
root-path suggestion is corrected for this new-document case.

Recommended change: start new projects in the remembered project directory or a
sensible personal TCAD folder. Keep the human document name separate from the
destination, and show the resolved save location clearly before writing.

### RR-05 — P1: first face click loses face selection

After creating the pedestal bore, with **Pick Faces** active, the first click on
the base top selected a Part1 occurrence. Clicking the same face again selected
**Face · DrilledHoles1** and produced the correct face highlight/context actions.
The reviewer reproduced the behavior, so it is distinct from the separately
qualified short-click automation issue.

Static diagnosis found that idle geometry selection changed the occurrence path
used by the viewport's pick-source fingerprint. The following synchronization
cleared face/candidate/popup state while retaining the selected component. A
narrow fix limits that source-changing occurrence path to input-body editors,
where it actually changes the pickable meshes.

**Recheck:** in the rebuilt app with **Pick All**, the first click on the base
face retained the face and context. This is positive runtime evidence for the
reported selection loss; the original Pick Faces mode and other selection/editor
types have not all been repeated.

Recommended outcome: one deliberate face click should retain that face,
localized highlight and applicable actions on the following frame. Check edges,
component selection and input-body editors too, without changing their scope.

### RR-06 — P1 release blocker: ordinary downstream holes hit ancestry budget

On the accepted plate + merged boss + central through bore, select the base top
face, choose Hole, place four mounting centers and Finish Positions. A Ø5 mm
Blind 10 mm hole feature reports:

> native partition ancestry exceeds its bounded serialization budget

Simple Through All and counterbore Ø10 × 3 mm attempts at those positions hit the
same diagnostic. No four-hole feature was committed. Cancel restored the earlier
part, removed the temporary Sketch4, and the accepted checkpoint saved.

This was a failure of an ordinary mounting-hole workflow on a small part. A
contained adapter-provenance accounting fix was included in the rebuilt package.
It is not described as missing general kernel capability, and raising/removing
work bounds alone is not an acceptance criterion.

**Recheck:** the same four positions were recreated after the already accepted
Fillet1. Ø5 Blind 10 mm preview validated; Ø5 Through All with Ø10 × 3 mm
counterbores validated, applied, showed one solid body and saved. This is a
successful downstream case on the later filleted stock. The exact pre-fillet
source target from the initial failure was not retested, nor was the standalone
simple Through All variant.

Remaining acceptance includes the original pre-fillet target, standalone simple
Through All and Undo; the final hole/chamfer checkpoint has now reopened. Preserve
explicit bounds and diagnostics for genuinely excessive work.

### RR-07 — P2: Fillet inherits an incompatible face-only pick filter

After face-based hole work, **Modify → Fillet** retained **Pick Faces**. The outer
rim could not be selected until the reviewer manually changed to **Pick Edges**.
After that change, one rim click expanded to the four native circular segments,
the 3 mm fillet preview was localized and valid, and Fillet1 applied and saved.

Recommended change: make a tool's required selection type available immediately,
or show a prominent filter-specific prompt with a direct action. Preserve an
intentional filter when it is compatible. This is selection-handoff friction,
not a failure of the accepted circular-rim fillet construction.

### RR-08 — P1 release blocker: default drawing title prevents first sheet

On **Roller Fixture - Bearing Pedestal**, **New Sheet** created the default
Sheet/front/top/right entries, but no drawing paper rendered. The 3D model stayed
visible behind the drawing controls. The viewport reported:

> Drawing interaction: Drawing: Title field 'DOCUMENT' exceeds its cell; enlarge it or reduce the text size.

Apply, PDF and DXF were disabled. The ordinary project name should not make the
default first drawing unusable. No drawing output has been accepted or exported
at that point. A narrow default-title layout fix was then included in a new
signed build.

**Recheck:** New Sheet now rendered a proper A4 sheet with front/top/right views
and the full DOCUMENT value wrapped within its field. Apply was enabled. This
accepts the title-layout case. The draft sheet was not applied or saved before
the separate responsiveness failure below.

Recommended change: make newly generated title cells fit normal document names
using adequate cell space and bounded wrapping/text size. Keep authored title
formatting editable and preserve clear validation for genuine overflow. A new
sheet should provide visible paper and a discoverable correction path.

### RR-09 — P2: selected long edge is not used when starting Edge Flange

Before Edge Flange, the reviewer picked a long base edge. The new feature instead
defaulted to **Edge1 = 120 mm**. Using **Pick base edge on model** then selected
**Edge4 = 700 mm** at the intended physical edge and the flange worked.

Recommended change: carry compatible current base-edge selection into a new
flange, show its name/length/extent immediately, and make fallback selection
explicit. This is a user-visible preselection/handoff observation; the report
does not infer which source or reference resolver caused it.

### RR-10 — P2: sheet-edge context menu omits sheet-metal actions

Clicking a sheet edge exposed Fillet/Chamfer and other groups, but no Sheet Metal
group. The reviewer could create sheet features through the normal commands,
yet the selection-first route did not expose the relevant fabrication tools.

Recommended change: offer applicable Edge Flange and terminal Hem/Jog actions
for recognized sheet edges, with clear unavailable reasons for unsuitable
selections. Keep general edge treatments available where supported.

### RR-11 — P1: Part1 labels hide which components are being inspected

The document and part were renamed **Roller Fixture - Sheet Guard**, but the
selection overlay still displayed **Part1**. The visible mismatch makes it harder
to tell which scope the user is selecting. This observation does not establish
whether the label represents a separate occurrence name, stale presentation or
another source identity. Later read-only inspection found that both the guard
and frame retain **Part1** as their standalone root assembly and occurrence name,
while their part definitions and documents have the intended names. That source
distinction explains an available Part1 label; a stale display cache is not
established.

Assembly insertion made the practical impact clearer: all three saved project
roots became linked subassemblies labeled **Part1**, and their reference
dropdown entries also read **Part1**. The reviewer had to distinguish frame,
pedestal and guard without useful names in those controls. Placement and one
mate still succeeded, but scope identification is needlessly ambiguous.

Recommended change: make definition versus occurrence names explicit, refresh
the correct display label after rename, and preserve distinct source identities.
The reviewer subsequently found the occurrence-name editor in an expanded
Components row. That is a usable correction route, but the accepted rename and
Save have now succeeded for Bearing Pedestal. Source inspection confirms the
saved name and the assembly reopened; all visible label states were not separately
recorded. The initial three identical names remain a clarity issue.

Later assembly inspection increased the severity: every one of the six result
labels read **Part1 / Part1**, including pairs involving the renamed Bearing
Pedestal. The reviewer could not identify which pair had a native error from
those labels. Saved-reference/source inspection shows each linked subassembly's
leaf occurrence is still named Part1, and inspection displays that leaf name.
Renaming its outer occurrence therefore did not fix these result labels.

Use a meaningful part/occurrence path with enough parent context to distinguish
repeated components. Reference IDs should remain stable, but users should not
have to inspect JSON to identify the two contacting pairs.

### RR-12 — P2: flat DXF could be easier for fabrication tools and operators

Read-only inspection of the exported guard DXF found exact straight outline
segments encoded as degree-1 SPLINEs and layer names based on UUIDs. The outline
was closed/connected and the three bends were present; no geometric error was
identified. This is an output-inspection observation, not a failed external
import or a runtime geometry defect.

Recommended follow-up: represent straight cutting paths as LINEs or a closed
polyline where possible, use readable cut/bend layer names while retaining stable
identity separately, and offer clear bend direction/angle annotations. Confirm
the result in representative CAD/CAM viewers before making compatibility claims.

### RR-13 — P1 release blocker: drawing controls become inaccessible

After the title correction, a new pedestal sheet rendered A4 paper with its views
and enabled Apply. Scrolling the drawing controls followed by an accessibility
read timed out after 60 seconds. Three subsequent application reconnection
attempts also timed out at 60 seconds each; the general desktop inventory still
returned in under a second. The draft sheet had not been applied or saved.

A native process sample strengthens this beyond a single automation timeout:
218 of 228 main-thread samples were in workspace UI, 118 in drawing controls,
and 97 in `current_value` paths rebuilding semantics and document fingerprints.
The paper worker was idle. These are stack-sample counts from this incident, not
a controlled performance benchmark or proof of one sole cause. The app was not
reported as crashed.

After the first targeted fix, runtime scrolling, scale editing, dimension creation
and Apply worked. However, computer-control calls still took 30–50 seconds, so
the reviewer did not consider the UI responsive. A second native sample found
remaining style-cascade work and repeated quadratic circle scans. A later
package included further corrections. Calls then improved to roughly 15 seconds
from 35–50 seconds, and drawing reopen plus PDF export worked. Final responsive
interaction remains unaccepted. These call durations still include
tooling overhead; the native samples are the additional evidence for
application-side work.

Recheck ordinary scrolling, field changes, Apply/Cancel and export on this same
small part before moving to larger assemblies. Cached UI values must remain
current while avoiding repeated complete work for every visible field.

### RR-14 — P2: default drawing scale makes the small part hard to read

The first A4 pedestal sheet selected 1:5 scale for a 120 × 80 mm part. The views
were visibly tiny relative to the available paper. Manual scale adjustment was
initially blocked by the separate responsiveness incident, but later changing
the sheet to 1:2 worked.

Source inspection found that initial layout divides the usable region into equal
cells, takes the smallest width/height fit with a 0.78 margin, then rounds down
to the 1/2/5 scale series. The tall top view combined with equal row heights is a
likely reason for the 1:5 result; that explanation is an inference from source,
not independent runtime acceptance of a different layout.

Recommended change: allocate initial rows/columns according to the actual view
extents, choose a useful standard common scale, retain orthographic alignment,
and expose easy shared-scale adjustment. This is a default layout/usability
observation, not a claim that the exported dimensional scale is incorrect.
The manual 1:2 adjustment has been observed; broader automatic layout is not
accepted by that result.

### RR-15 — P1 release blocker: drawing PDF action dispatches unknown command

After applying the drawing with its 120/80 mm dimensions, clicking **PDF** in the
drawing rail produced:

> Unknown command: file.export-pdf

No PDF was generated by that attempt. The applied drawing itself had reported
valid 25 ms; this was a UI command-dispatch failure, not an observed PDF
geometry/exporter failure.

**Recheck:** a newer package opened the PDF dialog and wrote the actual drawing
PDF. Its rendered page was inspected: page fit and title wrapping were correct.
Drawing DXF was also saved through the UI. Annotation-placement issues remain
separate from the corrected command route.

### RR-16 — P2: default dimension placement crosses geometry and caption

Smart Dimension successfully added 120 mm and 80 mm dimensions from top-view
rectangle corners. The default 80 mm annotation sat inside the part and crossed
the counterbores; the width extension crossed a view caption. The exported PDF
still showed both overlaps, confirming that the height select-and-drag attempt
had not moved it. This does not yet isolate whether the attempt missed its
handle or the intended interaction failed.

Recommended change: start new dimensions in a clear region outside the projected
part and reserve separation from view captions. Make the actual label/offset
handle easy to distinguish from reference geometry, and confirm direct drag
behavior before declaring the manual-placement recovery usable.

### RR-17 — P1: part-weld support buttons do not enter usable picking

On the four-member mitered frame, selecting miter top edge 26 and choosing
**Weld Bead** created a failed draft with a native crossing/endpoint certification
diagnostic. Switching to **Butt** retained the error. The support-pick buttons
did nothing, so the reviewer could not replace the supporting faces to establish
a valid joint. Cancel removed the failed weld and preserved the accepted frame.

Source inspection confirmed that the part controls dispatched an assembly
support-pick action and did not temporarily enable the required face filter.
A narrow UI correction was prepared. This was a confirmed support-selection
recovery defect; at that stage, the initial native construction error was not
isolated as valid-joint failure versus an unsuitable seam/support selection.
No claim is made that every weld fails or that a physical gap should be bridged.

Recheck the part's support buttons, visible selected supports, restored filter on
exit and a physically supported seam. Keep exact native validation and useful
contact error recovery, then inspect a successfully accepted visible bead.
Correction `cf3a078` was packaged, but the later seam-selection stall occurred
before those controls could be rechecked. Source completion is not runtime
acceptance.

**Later recheck:** after the selection correction, the pickers entered temporary
Faces mode, accepted faces 24 and 14 and restored Pick All. This accepts the support
selection handoff. Fillet rejected the parallel faces appropriately. However,
**Use butt/miter** cleared both supports; source inspection confirmed the
shortcut defect. Correction `1125e2a` is now packaged; the specific shortcut
runtime recheck was then completed in final `4e95e72`.
Re-picking the supports was possible, but native Butt construction still failed
as recorded separately in RR-26. The complete bead workflow has not passed.

**Final recheck:** frame reopened valid 107 ms; seam edge 63 → Weld Bead, support 1
face 24 and support 2 face 14 used temporary Faces and restored All. **Use butt/miter**
retained both exact support references and showed Butt/orientation following
joint faces. Evidence 21 records the state. Zero-trim toe validation remained;
Cancel restored valid 105 ms. This closes support-picking/retention for the named
case, not native bead construction.

### RR-18 — P1 production-output limitation: no hidden-line drawing mode

The rendered pedestal PDF shows front/right internal edges as continuous solid
lines. Source audit confirms no available hidden-line display toggle. The
[coverage inventory](../mechanical-cad-coverage.md) already identifies standard
views as wireframe and exact hidden-line removal as missing; this review now
shows its consequence on an ordinary machined part.

This limits production clarity: visible silhouette/edges and hidden internal
features cannot be distinguished through the expected removed/dashed modes.
It is not an export setting the reviewer failed to enable and not a new claim
that geometric dimensions are numerically wrong.

Recommended release work: implement per-view visible/hidden classification and
familiar hidden-lines-removed/visible controls, with consistent preview/PDF/DXF
output and preserved native associations. Until then, label the actual wireframe
scope clearly and do not describe these as complete production drawings.

### RR-19 — P2: cut-list headings do not state units or length reference

The frame CSV has readable member/section names and correct values against saved
path intent, but its headers say only **Length**, **Start angle**, and **Finish
angle**. They do not state mm/degrees or whether Length means reference-path,
section-centerline, shortest or longest saw-cut length. The exported profile
locating point is Top center, which makes a precise convention especially useful.

Recommended change: include units and an explicit length basis in headers or
metadata, and expose any separate long-/short-point fabrication lengths by name.
Keep nominal profile caveats and physical material separate from visual finish.
Four source rows with quantity 1 match the four saved members; optional equivalent
stock grouping is a convenience, not a missing-row defect in this output.

### RR-20 — P2: occurrence-name edits trigger full geometry evaluation

In the review assembly, editing an occurrence name in an expanded Components
row triggered a full geometry evaluation. One name-only result displayed
**3,449 ms / No material change**. The current Bearing Pedestal draft has the
correct text. The rename later applied and the assembly saved again. This
establishes avoidable workflow weight around naming. Later source inspection
confirms the saved Bearing Pedestal name, and the assembly reopened.

Recommended change: treat name/display metadata separately from geometry where
the authoritative dependencies allow it, and preserve normal undo/save behavior.
The timing is one visible evaluation result, not a performance benchmark.
Transient computer-control typing/paste behavior is not evidence of application
text loss.

### RR-21 — P2: component-pattern direction arrow obscures model area

The reversed-X pedestal pattern showed the expected green copy and applied, but
its direction arrow covered a large area of the model. It reduced visibility
of the geometry being positioned without indicating a construction failure.

Recommended change: use a compact, legible direction handle near the seed or
selected reference, with view-aware sizing and a clear reversal target. Keep
the actual copy preview dominant; preserve larger user-configurable handle
sizes where needed for accessibility.

### RR-22 — P1 release blocker: selecting a frame seam stalls the UI

After a clean `cf3a078` package reopened the saved tube frame with valid 110 ms,
the reviewer clicked a front miter seam with **Pick All**. Three UI observations
timed out at 30, 30 and 60 seconds. The main process, PID 19602, consumed about
105% CPU while both geometry workers were idle. A native sample was captured at
`/tmp/tcad-release-selection-sample-19602.txt` for source diagnosis. The saved app
process was terminated to recover. A retained copy is available as
[frame-selection process sample](/Users/tyler/tcad/release-review-2026-10-17/evidence/12-frame-selection-process-sample.txt)
(296,766 bytes); this diagnostic was copied from the incident, not collected by
the report scribe.

This is a confirmed selection/UI stall, separate from the earlier unsupported
weld construction. The timing and worker state did not identify the exact cause
on their own; diagnosis was pending at that point. No new weld geometry was accepted or native
kernel failure established by this incident.

Release acceptance requires ordinary seam/edge selection on the saved frame to
remain responsive, retain a clear selection and permit Cancel/Escape. Recheck
the corrected weld-support picking only after this selection path is usable.

**Recheck:** rebuilt `633fa2f` returned the context menu for an ordinary frame
miter-edge click in 9 seconds, instead of the previous repeated timeouts.
This accepts the ordinary-click recovery. Explicit chain expansion was not
rechecked. The 9-second value includes computer-control overhead and is not a
native interaction benchmark.

### RR-23 — P1: switching theme makes Settings actions inaccessible

The reviewer saved Nocturne in Application preferences, quit and restarted; the
theme persisted. In **Settings → Themes**, selecting **Classic** changed the
style but made the modal exceed the window height. Its title and OK/Cancel/Apply
actions were offscreen; an attempted OK click reported
`cannotClickOffscreenElement`. This is supported by the observed dialog bounds,
not classified from the tool error alone.

Escape successfully discarded the unapplied switch and restored Nocturne. No
settings loss or inability to recover is claimed. Correction `f1b3fcf` was
packaged for recheck.

**Recheck:** Nocturne → Classic in Settings kept the title, footer and
OK/Cancel/Apply visible. Clicking OK saved the preferences. Evidence 14 shows
the corrected bounds. Smaller-window and other density/theme combinations
remain unexercised.

Recommended outcome: every supported theme/density change keeps the dialog
header and Apply/OK/Cancel reachable, with scrolling confined to appropriate
content. Recheck both switch directions and a smaller window. Treat current
theme-driven size as input to bounded layout, not a reason to lose its controls.

### RR-24 — P2: ordinary theme choice leads too easily to source editing

**Workspace → Layout and Theme…** immediately opened very large JSON/CSS editors.
The actual theme picker is under **Tools → Settings → Themes**. Both routes can
be useful, but their labels do not make the normal versus advanced task clear.

Recommended change: provide a clearly named Theme/Appearance route to the picker
and label direct source editing **Advanced layout and style…** or equivalent.
Keep full user styling available without presenting implementation details as
the first step for choosing a theme.

### RR-25 — P2: Nocturne Publish controls need layout refinement

The reviewer completed Publish Settings and Publish in Nocturne, but the panel
required much scrolling and showed label overflow. No lost output choice or
blocked publication action was reported for this panel. The exact overflowing
label/control was not isolated in the observation.

Recommended change: give repository/branch/project scope a compact, readable
group, bound long labels and paths within the panel, and keep primary actions
easy to reach while preserving user-defined sizes. Review the same form in
Classic and Nocturne with realistic local paths and repository URLs.

### RR-26 — P1: a supported miter/Butt bead remains invalid

On the frame miter seam, the corrected pickers accepted top support faces 24 and 14.
Those parallel supports appropriately cannot form a Fillet bead. After choosing
Butt and re-picking both faces, the reviewer tried width **3 mm**, crown
**0.5 mm**:

| Trim setting | Observed result |
| --- | --- |
| No trim | Toe footprint outside the support |
| Start trim 3 mm | Prismatic crossing with no coordinate enclosure |
| Both ends trimmed 3 mm | Unsupported prismatic arrangement; near-parallel lines neither certified coincident nor separated |

Apply stayed disabled. Evidence 15 records the supported miter failure. Cancel
restored the frame with valid 107 ms, and no invalid bead was saved. This is
stronger evidence than the initial missing-support attempt: explicit supports
are now accepted, and the ordinary Butt/miter construction remains blocked for
these bounded inputs. It does not establish that every weld type or seam fails.

Recommended next work: resolve this saved seam/support construction with native
geometry diagnostics, preserving support/contact checks and clear trim recovery.
Recheck the actual visible bead and saved reopen after correction; do not count
a picker fix or a different unsupported-case message as a completed weld.

### RR-27 — P2: material and finish controls do not expose one clear task

Single-clicking **Material Aluminum** in the tree had no visible action. Clicking
**Finish** opened the right appearance editor. **Engineering materials** then
opened a left material editor while keeping the right appearance editor visible,
with duplicate Apply/Cancel actions. A material assignment still succeeded, so
this observation does not establish that material editing is broken. Source
audit found the tree item updates `object_selected` without opening its editor;
both visible editors use the same shared document draft. This is a source
explanation of the behavior, not evidence of conflicting saved transactions.

Recommended change: give material and visual finish explicit entry points and
scope labels, and make the active shared draft clear. When both panels are
visible, show that their repeated actions apply/cancel that same draft. The successful
steel assignment leaving the optical finish unchanged supports the intended
separation of physical material and appearance.

### RR-28 — P1: lighting rotation blocks a valid finish draft

After a valid **RAL 1015 Light Ivory Matte** Part Definition preview, changing
lighting rotation from **0 to 45** left Apply disabled with **Correct highlighted
value** and **Draft changed—evaluating preview**. Tab and Return did not resolve
it. Evidence 16 records the stalled state. The finish had not been applied; the
reviewer canceled that draft. The earlier engineering-material assignment
was committed separately and later saved as steel.

Source diagnosis found a false validation error caused by null-environment
versus typed-serialization mismatch. This is a source explanation rather than
proof of indefinite geometry-worker computation. Correction `4e95e72` was
packaged for recheck.

**Recheck:** on the STEP round-trip project, built-in studio rotation **0 → 45**
showed **Valid proposed geometry** with Apply enabled. Apply reported one undoable
change, and File Save succeeded. Read-only source confirms 45°. This accepts
the built-in rotation/commit/save case. The earlier simultaneous RAL-finish
draft was not repeated; HDR import, actual Undo and reopen remain unexercised.

Recommended outcome: a normal environment rotation should validate promptly,
preview and apply; absent imported HDR must remain a supported default. Recheck
rotation, Apply, Cancel and saved lighting state after correction. HDR/EXR and
image texture import remain unexercised.

### RR-29 — P1: assembly BOM controls remain too slow

The four-active-occurrence review assembly reopened and its BOM quantities were
correct. However, the BOM panel had a 30-second computer-control timeout followed
by a 51.7-second reconnect, and a 45-second return after export Save. The CSV
export itself succeeded. The subsequent source audit is summarized below.

The source audit found repeated per-frame BOM/cut-list generation without native
kernel recomputation. Current-value reads rebuild controls and the whole-document
fingerprint, with another hash for each row. This supports a UI-work diagnosis;
the suggested safe caching work remains future implementation, not a completed
or runtime-accepted fix.

These values include computer-control overhead; they are not isolated UI frame
timings. Together with the reviewer's inability to interact promptly, they are
an open responsiveness finding, not a BOM arithmetic failure. Recheck opening,
scrolling, selection and export on this saved small assembly before declaring
the inspector usable at larger assembly scale.

### RR-30 — P1: native interference results are unavailable at both supports

The saved four-target assembly inspection reported all six pairs, but the two
pedestal/frame contacts had **Native interference check unavailable** with a
parameter-domain division error on face 1 / operand 0 and unresolved parameter-loop
orientation. Both approximate boundary distances were zero. Four other pairs
were reported separated. The inspection definition applied and saved; accepting
the report definition is not certification of the unresolved pairs.

Read-only saved-reference mapping identifies those pairs as the near and far
pedestals seated at the frame's top z0 plane. That is consistent with expected
contact, but neither the saved transforms nor approximate distance prove absence
of overlap. Native contact/interference completion remains blocked for this
ordinary review layout.

Recommended next work: use the saved inspection and target pairs to resolve
contact classification while preserving honest unavailable/uncertain states.
Provide useful names and pair highlighting so an operator can inspect the
reported geometry. Do not substitute approximate zero distance for a certified
non-interference result.

### RR-31 — P2: linked-source update presentation hides the useful change

The frame source update succeeded, but all **Check/Preview Part1** actions were
grouped above three source paths, making it hard to associate a button with the
intended component. The Source changes preview showed raw JSON pointers, IDs
and SHA metadata instead of a plain description of the material change.

Recommended change: place each source name/path, current/new revision and its
actions together. Lead the change preview with engineering facts such as
**Tube Frame material: Aluminum → Structural steel S355J2H**, then show affected
instances, mates, drawings and BOM rows. Keep raw source/hash details in an
expandable advanced section. Preserve the successful explicit Check → Preview
→ Apply transaction.

### RR-32 — P2: accepted STEP import leaves an empty-part prompt on the model

The pedestal STEP preview and Confirm Changes succeeded, but the active part
remained empty Part1. **No features yet** appeared in the tree and **Start your
first sketch** covered the imported geometry. The imported Roller Fixture -
Bearing Pedestal part was available in the part selector beside Assembly/Part1.

Recommended change: after a new-document import, focus the imported part or
assembly and fit its geometry. Make retained empty parts explicit and remove
the first-sketch prompt when it does not describe the visible active task.
This is an active-scope/UI handoff problem; visual import succeeded and no
geometry loss is established.

**Workaround observed:** choosing Roller Fixture - Bearing Pedestal in the part
selector removed the empty prompt and showed Solid Bodies(1)/Pinned STEP solids.
The imported project then saved separately.

## Tooling qualifications

Default short synthetic clicks frequently had no effect. Holding a click for
about 150 ms worked in the observed UI. This is recorded as a computer-control
timing limitation, not a proven defect in ordinary human clicking.

Accessibility `setValue` and `selectText` attempts in the name editor did not
change its text or selection; confirming Rename left Part1 unchanged. Synthetic
Cmd/Super shortcuts entered bare letters in the observed sequence. Ordinary
typed text did enter the field. A later triple-click followed by typed replacement did
rename the document successfully. These are recorded as automation entry issues,
not evidence that ordinary human renaming or shortcuts are broken.

The Nocturne-switch tool call timed out, but the next inspection showed the
responsive themed UI. The tool timeout alone is not classified as an app hang.
One fillet interaction also timed out in computer-control tooling; reconnection
showed the valid accepted fillet. It is not reported as an application crash.

During frame work, edge-context-menu calls took 41–50 seconds and weld-field calls
16–26 seconds. They completed, but the reviewer still found the workflow heavy.
These are end-to-end tooling-call durations, not isolated CAD response timings;
the remaining frame-context/field cost needs measurement rather than being
accepted as responsive from the successful structural-member evaluation alone.
During the later supported-weld attempt, selection/field calls still took
30–38 seconds. Source correction `1125e2a` removes repeated full semantic rebuilds
per field. Final `4e95e72` field operations improved to about 13–19 seconds but
remained sluggish. This is neither a complete performance pass nor an isolated
native input benchmark; the values remain tool round trips.

The recorder check showed a clean app-window capture without audio. The first
task capture failed: its metadata reports zero bytes and no corresponding movie
exists. It is not delivered feature footage. The user deferred videos, so no
replacement recording was made. The hands-on review has now stopped.

## Release evidence not completed

- Broader sketch geometry, relations, Smart Dimension selection paths and handles;
  only the first dimensioned rectangle and local dimension Cancel have run.
- Further solid-modeling variants and shallow pockets; named counterbore,
  rim-fillet and adjoining-chamfer cases have now succeeded and reopened.
- Sheet DXF external-viewer import and guard reopen, a valid visible weld,
  mixed structural members and broader assembly mates/updates, plus
  larger drawings. The first part sheet now saves, reopens and
  exports; annotation placement and drawing responsiveness still need work. Guard
  stock/flanges/hem/flat/formed state, Save and export status have been observed.
- Guard reopen and resource portability for accepted projects.
- Theme switching on active tasks and smaller-window/density layouts; Nocturne
  Save/restart, Escape recovery and corrected Nocturne → Classic dialog bounds
  with OK have been observed.
- STEP precision/units comparison and imported-project reopen, drawing
  DXF external import, hosted Git/credentials, publication retry and
  conflict safety, remembered-settings reopen and republish, optional output
  variants and README preview. First local Publish and generated artifact
  inspection have now succeeded for the saved pedestal.

Documented broader scope limits remain in the checklist's static-risk section.
They are separate from the observations above and have not been rediscovered by
this session.
