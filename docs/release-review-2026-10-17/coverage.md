# Release review coverage — October 17, 2026

This is the manual review plan, not a report of completed checks. The user has
explicitly requested hands-on release review and recordings; that request
supersedes the older instruction to leave interactive acceptance to the user for
this session. No automated code tests are proposed here. The main reviewer owns
the desktop session. This checklist was prepared from repository documents and
earlier reports without operating TCAD.

Source checkpoint when this plan was prepared:
`c6b7343bb938db64c285ce691286dd5ed327bf73`. Record the actual app build, theme,
macOS version, display size/scale and input device in the results. Do not assume
that the running bundle matches this commit.

## Evidence and release decisions

All rows started **Not run**. Current observations are recorded separately in
[report.md](report.md), with capture status in [recording-index.md](recording-index.md).
This checklist remains the planned coverage; a listed row is not itself a pass.
The October 17 session is now closed. Five review projects and the bounded
results are preserved in the report. The full matrix was not completed; rows
without a recorded observation remain Not run. Videos were stopped and deferred
at the user's request.
Use these statuses in the results ledger:

- **Pass:** performed in the UI; the expected result was seen and saved where
  appropriate. Include project path and screenshot or video time.
- **Fail:** performed; observed behavior did not meet the stated check. Record
  exact inputs, recovery and reproducibility.
- **Partial:** only the named subset was exercised, or it required a workaround.
- **Blocked:** an earlier failure prevented the check. Identify that failure.
- **Not run:** no runtime evidence. A visible command or successful compilation
  is not a pass.
- **Known limit:** documented scope limit from source inspection. Keep this
  separate from a newly observed failure.

Priority **P0** means possible lost/corrupt data, wrong manufacturing output,
unintended disclosure or an unrecoverable application state. **P1** blocks an
ordinary supported workflow or hides the next required action. **P2** covers
broader capability, efficiency and polish. Priority describes impact, not proof
that a defect exists.

For each attempted operation, observe selection, live preview, validation,
Apply, Cancel, Undo and saved reopening. A feature that creates once but cannot
be edited or reopened is not fully accepted. For repeated variants, record the
specific geometry and modes; do not mark a whole family complete from one case.
Save a checkpoint before risky work. Preserve failed drafts/reproducible inputs
separately from the accepted demonstration parts.

## Proposed engineering project

Build a **guarded roller inspection fixture**. It combines ordinary fabricated,
machined and purchased parts in a useful assembly, without depending on exotic
freeform modeling. Dimensions below are review targets, not a certified machine
design or a load rating.

| Deliverable | Proposed intent | Main coverage |
| --- | --- | --- |
| Bearing pedestal | 120 × 80 × 12 mm plate, Ø40 × 25 mm boss, Ø20 bore, four Ø5 holes with Ø10 × 3 mm counterbores, two shallow pockets, 2 mm adjoining chamfers and 3 mm circular-rim fillet | Sketch dimensions, face-attached join, multiple contours, cuts, holes, edge treatments, materials and part drawing |
| Roller and shaft | Hollow Ø60/Ø54 × 650 mm roller; stepped shaft with bore/shoulder and an axis-driven circular feature | Revolve, thin/nested profiles, axis references, patterns, concentric mates and configurations |
| Sheet guard | 700 × 120 × 2 mm blank, two 25 mm returns, one 10 mm open hem, base-normal mounting holes; optional jog in a saved branch | Sheet stock, base/terminal edge selection, cut after bends, Formed/Flat and bend output |
| Welded stand | 900 × 700 mm rectangular top, four 800 mm legs and lower rails; square tube, flat-bar brace and round cross member; one gusset, end cap and visible supported bead | 2D/3D layout sketches, library profile location, mixed groups, trim, fabrication identities and cut list |
| Fixture module | Stand, two pedestal definitions/instances, roller/shaft, guard and representative M6 hardware | Shared definitions, ground/mates, placement, hierarchy, appearance overrides, BOM and inspection |
| Fixture line | Three nested fixture modules at 1100 mm pitch along a picked frame edge | Nested links, component pattern direction, large tree, drawing responsiveness and coordinated updates |
| Review drawing pack | Pedestal sheet with dimensions/counterbore callouts; guard flat sheet/bend table; stand sheet/cut list/weld symbol; assembly sheet/BOM/balloons and an exploded view | Production views, annotation association, tables, PDF/DXF and Publish |
| Variant and change | Wide module configuration; change pedestal mounting-hole spacing in a source branch; preview one linked update and transfer one selected feature | Configurations, references, Git versions/forks/update control, export freshness |

Save each main part as an independent project before inserting it. Keep a local
assembly project and its linked-source projects together under the review folder.
Use meaningful part numbers and names. A small separate **feature sampler** may
cover surfaces, direct face editing and tooling without adding irrelevant
features to the fixture. Keep one clean accepted checkpoint for each video.

The earlier three-part and conveyor projects should also be reopened as
regression sources. Do not overwrite their original saved evidence.

## First gate: opening, navigation, theme and save

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| R01 | P0 | Open the packaged app, then an existing saved project. Close/reopen the saved review checkpoint. | A usable window, correct project/part names, valid accepted geometry and features; no blank launch, silent fallback model or lost data. |
| R02 | P1 | New Part with no content libraries installed/enabled. | Empty Part1 with named planes/origin, useful commands, plain fallback appearance and no unsolicited demonstration model. |
| R03 | P1 | Orbit through both camera poles and a full turn; pan, pinch/zoom, fit, Normal To and named-view recall. | Intended direction, no angular stop, geometry remains framed, no accidental sketch edits or menus. Qualify synthetic gestures separately from real hardware. |
| R04 | P1 | Click face, straight edge, circular rim, sketch member and empty space; Cmd-click two references. | Persistent, localized selection highlight; correct type/count; context menu only for geometry; empty-space deselection is clear. |
| R05 | P1 | Hover near straight and circular edges, then leave. | Appropriate midpoint guides/center axes and marks appear predictably without turning into permanent source geometry or stealing selection. |
| R06 | P1 | Use active-part, edge, face and sketch filters; cycle overlapping candidates in an assembly. | Candidate type and occurrence visible; ghost context excluded normally; explicit reference mode works; Tab does not also move toolbar focus. |
| R07 | P1 | Inspect left manager idle, in sketch, in a feature, and in assembly/drawing modes. Resize narrow and wide. | Clear section dividers, active task first, selected references compact, one clear completion group, primary inputs accessible without hunting. |
| R08 | P1 | Inspect normal, hovered, selected, disabled, pending, invalid and keyboard-focused buttons/fields in Classic. | Buttons look clickable; labels fit; states differ by more than subtle color; error styling does not label incomplete setup as failure. |
| R09 | P1 | Classic → substantially different installed theme → Classic. Repeat with a selected object and a pending valid parameter edit. | No unsupported-style-property failure; same source, selection and pending value; usable layout and completion actions in both themes. |
| R10 | P1 | Change density, rail width, button size and theme preview; cancel settings, then apply once. | Compact yet readable content, content-fitting labels and correct Cancel/Apply scope. Ordinary project parameters do not change with theme. |
| R11 | P1 | Import/reload a personal theme with a small visible layout/style change; use invalid copy and restore default. | Changes affect the named controls; bad copy reports a useful error while last usable/default layout remains recoverable. Do not corrupt user's original theme. |
| R12 | P1 | Search commands by familiar name; navigate with keyboard; open tooltips and contextual secondary groups. | Results have meaningful target names, available actions are discoverable, disabled reasons explain the next step. |
| R13 | P0 | First Save, Save As with a new name/folder, Cancel Save As, reopen both accepted saves. | Human name and location are clear; original remains unchanged; resources stay with each project; no phantom success on failure. |
| R14 | P0 | Exercise unsaved close/new/open prompts, Undo/Redo and recovery using disposable checkpoints. | Pending work is not silently discarded; recovery source and destination are clear. Do not terminate an unsaved user project. |
| R15 | P1 | Change global navigation/default appearance preferences versus model units/material; cancel and apply. | Personal and model scope are clear, persistence matches scope, model edits make one undoable transaction. |

## Sketching and constraints

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| K01 | P1 | Sketch on Top, then a model face and a datum plane in the same viewport. | Actual support is highlighted, normal view frames useful geometry, plane/source is named and editing stays in one workspace. |
| K02 | P1 | Draw origin-anchored rectangle, concentric circles and an independent open chain. | Inferred origin relation is visible and persists when resized; unrelated open geometry does not corrupt closed-profile selection. |
| K03 | P1 | Tool-first Smart Dimension: circle center to straight edge, then enter value. Repeat with Cmd-click preselection. | Correct two references, centered/perpendicular witness, stable value-entry position, expected geometry movement and editable driving value. |
| K04 | P1 | Cmd-click several lines for Horizontal/Vertical and Equal; circles/arcs for equal radius; select tangent pair. | Whole eligible selection receives relation, Tangent is discoverable with an applicable disabled reason, one Undo reverses batch. |
| K05 | P1 | Constrain points to Origin/axes and one another; apply coincidence, midpoint, parallel, perpendicular and symmetry. | References are visibly clear; constraint/dimension change drives expected entities; no silent selection of a different point. |
| K06 | P1 | Drag free, partly constrained and fixed handles. Edit a driving value, change to reference, then cancel. | Freedom is truthful, fixed geometry stays fixed, current preview updates, invalid text stays editable, Cancel restores start. |
| K07 | P1 | Introduce a deliberate redundant/conflicting dimension on a disposable copy, inspect diagnostics and recover. | Participating members/relations highlighted; useful next step; no false fully-constrained claim after failed solve. |
| K08 | P1 | Draw multiple closed islands with nested holes plus an open construction path; select one/all material regions. | Region selection is clear and follows authored entities; construction/open chains excluded correctly; no hidden primitive replacement. |
| K09 | P2 | Cover corner/center/three-point rectangles, line/polyline/centerline/midpoint line, point, circle and arc modes. | Placement prompts, cancel behavior, handles and saved edits make sense for each attempted mode. |
| K10 | P2 | Create straight/arc slot, polygon, ellipse, conic, interpolating spline and control-point spline. | Exact attempted modes create, edit and reopen; invalid degeneracies recover cleanly. Record unattempted variants explicitly. |
| K11 | P1 | Convert supported model edges, offset a line/arc chain, add sketch fillet/chamfer; change any member to construction. | Source references/offset/radius editable, expected closed regions remain, source edits update linked conversion or give explicit repair. |
| K12 | P2 | Trim/extend/split; move/copy/rotate/scale/stretch; undo each. | Trim preserves intended side, source intent is clear before destructive detachment, selection and constraints remain coherent. |
| K13 | P2 | Mirror/dynamic mirror, linear/circular sketch pattern, suppress one instance. | Source versus generated geometry is clear, counts/reversal update predictably, no duplicate closure at 360°. |
| K14 | P2 | Create/import a sketch block, insert twice, edit source, detach one, save/reopen. | Shared and unique scopes clear; retained references; portable block and catalog labels usable. |
| K15 | P2 | Add sketch text with a pinned font and a polynomial equation curve on a sampler. | Source/size/placement edit, portable reopening, supported exact downstream use; unsupported shaping is explained. |
| K16 | P1 | Create 3D line/arc path for frame, apply world-axis and distance/equal constraints, drag handles. | Spatial working-plane/XYZ intent clear, depths visible, no accidental flattening; supported path consumed by structural/sweep feature. |
| K17 | P2 | Create ordinate/baseline/chain dimension group and edit spacing/labels. | Datum/targets remain associated; driving/reference modes and group Cancel/Undo are coherent. |

## Solids, references and surfaces

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| M01 | P1 | Plate extrusion and face-attached boss with Merge Result; edit height and footprint. | One intended solid, correct new-material highlight, no coincident ghost body, downstream references follow or fail explicitly. |
| M02 | P1 | Extrude/cut multiple closed islands and holes, with selected contours and All Closed Contours. | Correct islands and holes included, unrelated open chains ignored, body scope explicit. |
| M03 | P1 | Blind, through-all, midplane/two-direction and supported up-to-face extrusions; reverse and start offset. | Direction/end condition evident in preview, depth in chosen units, Cancel restores stock. |
| M04 | P1 | Bore, four counterbores, countersink and tapped-hole intent on pedestal; edit center/diameter/depth. | Physical heads/bores correct, hole positions understandable, thread is explicitly cosmetic, drawing callout follows design. |
| M05 | P1 | Existing-bore counterbore and two shallow pockets after boss/bore/edge treatments. | Earlier open-boundary/endpoint failure does not recur for the documented supported input; accepted geometry reopens. |
| M06 | P1 | One edge, complete rim, adjacent planar chain and mixed independent fillet/chamfer selections. | Selected extent clear, intended rounded/beveled faces, removed material highlighted, edge removal/focus works, unsupported junction preserves stock. |
| M07 | P1 | Shell an ordinary box or supported rounded prism; remove cap and change thickness/direction. | True cavity/opening, thickness/selection clear, no axis-aligned-only surprise for documented support, invalid thickness recoverable. |
| M08 | P2 | Stepped revolved shaft and cut revolve; thin extrusion/revolve. | Axis source/reverse/angular limits and resulting solid clear; no center-axis singular artifact. |
| M09 | P2 | Sweep supported section along line/arc path; change start/reverse/roll; loft two unequal-edge sections. | Section/path roles visible, stable seam/direction, no twisted or folded artifact; actual supported source saved. |
| M10 | P2 | Solid loft with three compatible sections and endpoint controls; surface loft with line/arc/spline sections. | Section order and connectors clear; previous edge-count/injectivity problems classified by actual result, not assumed fixed. |
| M11 | P1 | Part pattern directed by picked sketch line, datum axis and model edge; add second grid direction and reverse. | Reference/arrow visible, pitch/count correct; source direction edit updates copies. |
| M12 | P2 | Circular, curve and placement/body patterns; mirror body; skip one supported instance. | Stable count/identity after edit, clear body rather than feature replay scope, supported axis/path selection. |
| M13 | P2 | Union/subtract/intersect/split multibody stock; keep/delete bodies and change output scope. | Body counts/selected members accurate, intended empty results explained, no unrelated bodies consumed. |
| M14 | P2 | Rib to supported face, planar draft and Move/Offset/Delete-Heal/Replace Face sampler. | Correct limits and references, preview localized, dependent history retained; unsupported carrier rejected clearly. |
| M15 | P1 | Create offset/angled datum plane, axis, point-on-edge and coordinate system; use them in downstream features. | Names, source/parameter direction, placement and local units clear; source edits propagate. |
| M16 | P2 | Reference curve/composite/helix sampler and supported curve pattern. | Path intent and supported spacing explicit; curve is reusable native reference, not a drawn display substitute. |
| M17 | P1 | Select cached History station, return current; edit/reorder feature; suppress via configuration. | Display matches chosen station without mutating source; active historical state obvious; invalid reorder rejected without loss. |
| M18 | P1 | Deliberately retire a referenced face/member on a copy; use reference repair. | Unresolved reference is identified, valid old display labeled, replacement preview/Cancel/Undo works without guessing. |
| M19 | P2 | Planar, extruded, revolved, ruled and filled surface sampler; knit supported boundaries, trim, reverse and thicken. | Surface versus solid clear; each accepted edit and export reflects real geometry; limitations not hidden by command names. |
| M20 | P2 | Surface offset, domain trim/untrim and supported mid-surface sampler. | Parameters/domain/side selection understandable; actual holes/boundaries preserved; uncertain output is rejected. |

## Sheet metal and welded fabrication

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| F01 | P1 | Base Flange/Tab from closed sketch with thickness, inside radius and K factor. | Stock direction and source clear; thickness persists through later edits. |
| F02 | P1 | Two opposite base-edge flanges, then terminal attachment and open hem; optional jog. | Explicit base versus terminal pick, highlighted source, sign/angle/height understandable, no numeric-endpoint hunting. |
| F03 | P1 | Two base-normal holes after flanges/hem; change diameter and reopen. | No ancestry exhaustion for the ordinary supported case; holes appear in formed and flat stock. |
| F04 | P1 | Switch Formed/Flat repeatedly; export selected sheet flat DXF and add bend table. | State obvious, no growing history clutter, selected source correct, cut contours and bend lines available. |
| F05 | P1 | Structural Member: pick path entities, choose installed profile/type/size, click locating point and roll/offset. | Familiar sketch→members→profile workflow, position diagram clear, actual members preview; no accidental custom-section workflow. |
| F06 | P1 | Append square tube, flat bar and round section groups; miter and butt corners. | Group/profile scope and inherited stock clear, native body count truthful, member names and sizes readable. |
| F07 | P2 | Trim/Extend to datum/face, add gusset and end cap, edit gap/end offsets. | Affected ends highlighted, real stock remains separate, cut-list length/angle/accessory rows update. |
| F08 | P1 | Add a supported visible part weld using seam plus two real support faces; repeat assembly-owned bead. | Supports/root/toes highlighted, size/ends editable, bead is visible real geometry and appears in export without changing shared part unexpectedly. |
| F09 | P1 | Try physically invalid free-edge weld on disposable copy, then replace support or cancel. | Specific contact error/recovery, no fake bead bridging missing stock, no duplicate diagnostic wall. |
| F10 | P1 | Cut-list CSV/drawing table with mixed profiles, quantities, lengths, end angles, material/finish; suppress/restore member. | Human labels and stable item numbers, correct scope and quantity, preserved reservations and no UUID-only section name. |

## Assemblies, reuse, inspection and variants

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| A01 | P1 | Insert independent parts and a nested module; single-click occurrence, double-click/Edit definition. | Selection versus definition editing distinct; breadcrumb, component name and parent scope match visible geometry. |
| A02 | P1 | Ground base, Move/Rotate component with numeric fields and handles, cancel, then apply. | No unexplained jump; units and frame clear; handle owns gesture without moving camera; one Undo reverses accepted placement. |
| A03 | P1 | Concentric roller/shaft, planar/coincident support, distance and angle mates. | Correct occurrence/reference highlighting and solved pose; fixed/free state and remaining motion visible; conflict doesn't silently reposition unrelated parts. |
| A04 | P2 | Width/center, limit, hinge and one supported gear/screw/slot/path mate sampler. | Expected finite motion, signs and reference frames clear; documented continuous-turn limitation not called a pass. |
| A05 | P1 | Repeated shared pedestal, make one unique, edit shared source, isolate/ghost and reference-pick. | Shared versus unique scope explicit; intended instances update; surrounding ghost geometry doesn't steal ordinary picks. |
| A06 | P1 | Component line/axis/edge direction pattern with second direction; edit count. | Seed shown before copies, transformed nested direction correct, natural tree order and stable names. |
| A07 | P2 | Copy With Mates, rigid group, suppress/re-anchor group. | Original/duplicate placement and internal mates correct; outside endpoints only rebound deliberately. |
| A08 | P1 | Expand/search/collapse three nested modules and many patterned components. | Hierarchy preserved, no flattened repeated Part names, active selection retained, task controls responsive. |
| A09 | P1 | Install and insert representative screw/washer/nut; adjust screw length, reuse same definition. | Size and simplifications clear, quantity/BOM identity correct, no silent duplicate definition on unchanged insertion. |
| A10 | P1 | Display state hide/ghost/finish, saved exploded view and connector lines; bind drawing sheet. | Presentation doesn't move mechanical source or change mass; selected state restores; exported drawing uses chosen state. |
| A11 | P1 | Measure length/diameter/angle and inspect mass properties with steel/aluminum assignments. | Units/source/configuration clear; density affects mass independently of visual finish; incomplete density doesn't present false complete mass. |
| A12 | P2 | Interference/clearance inspection on touching, separated and deliberately overlapping parts. | Exact versus tessellated/uncertain status explicit; affected pair highlighted; no false certified clearance claim. |
| A13 | P1 | Narrow/wide configuration with dimension/count/child configuration overrides; edit base. | Inheritance visible, source instances stay distinct, geometry/BOM/drawing use selected variant, no accidental base mutation. |
| A14 | P2 | Configuration CSV/table round trip, shared equation, design monitor and simple tolerance stack. | Units/dependencies clear, invalid/cyclic equation recoverable, output identifies assumptions rather than claiming inspection certification. |
| A15 | P1 | Change linked pedestal source, check for updates, preview impact, cancel and then accept. | Old pin usable offline; check alone does not change model; accepted update is one undoable operation and other assemblies don't change silently. |
| A16 | P2 | Tooling sampler: draft analysis, shrink, planar shut-off/parting strip and simple core/cavity split. | Supported planar scope explicit; analysis doesn't manufacture bodies; preserved part and distinct tooling outputs. |
| A17 | P1 | Evaluate assembly BOM/cut list and export CSV with patterned parts and pinned linked definitions. | Active quantities and names agree with configuration; suppressed/reserved pattern instances are excluded; material comes from the accepted pinned source, and controls remain responsive. |

## Appearance, resources and content repositories

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| L01 | P1 | Open each missing-library screen with packs disabled; use relevant download/manage action. | Short explanation and usable fallback, sane destination `~/TCAD/Libraries`, source repo visible; current draft preserved. |
| L02 | P1 | Download/register/enable a profile, texture/finish, theme, material and hardware pack. | Entries appear in their normal controls, status and retry understandable, install does not silently change current model/theme. |
| L03 | P1 | Offline/missing permission/occupied-folder download case on disposable destination. | Specific error, retry/cancel accessible, no overwrite of existing checkout or live source. |
| L04 | P2 | Create a local data library, add entry, inspect changes/save version; fork and disable original before enable. | Source/destination/identity conflict clear; unrelated/staged work preserved; no unexpected automatic publishing. |
| L05 | P1 | Inspect library update, cancel then apply a benign disposable update. Disable/remove registration. | Review identifies changed files, existing pinned parts remain usable, source files aren't deleted by unregister. |
| L06 | P1 | Assign engineering material and independent clear-anodized, painted and dark finishes to parts/occurrences/faces. | Visual versus physical roles clear; mass unaffected by finish; override scope visible; selection doesn't wash out whole model. |
| L07 | P1 | Import grayscale height image; adjust physical scale, direction and height; try roughness/normal maps. | Grain follows selected image, not procedural stripes; map slots understandable, close-up and overview remain readable. |
| L08 | P1 | Lighting/exposure/environment settings, bright and dark finish, edges on/off. | Reflective metal has readable planes/curves; edges default on; ghost/selection/preview remain distinct under both themes. |
| L09 | P1 | Save textured part, move external import source out of reach, reopen saved copy. | Pinned image stays portable; corrupt/missing project image reports repair rather than a false successful render. |
| L10 | P2 | Import HDR/EXR, change while decoding, cancel; missing theme checkout fallback. | Stale import not applied, preview responsive, prior environment/theme recoverable, status identifies fallback. |

## Drawing and manufacturing output

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| D01 | P1 | Create part and larger nested-assembly sheets; pan/zoom/change sheet while projections prepare. | Responsive controls and cancel; no frozen manager; one viewport paper state persists after Apply. |
| D02 | P1 | Front/Top/Right/isometric views, parent alignment, scale, paper dragging and resize handles. | Standard views align at consistent scale, selection visible, fit and snapping predictable, no debug/provenance text on paper. |
| D03 | P1 | Add linear, point-to-point, parallel-edge, angle, diameter/radius dimensions and hole/thread callouts. | Correct actual references and units, readable witnesses, position controls stable, source change updates value or explicit repair. |
| D04 | P1 | Add dimension tolerance, datum and feature-control frame; edit on paper and in manager. | Same current values/associations, no clipped symbols or unsupported standards claim. |
| D05 | P2 | Ordinate/baseline/chain dimensions, datum target, surface finish, weld symbol and revision cloud. | Stable labels/leaders, meaningful attachment repair and shared paper/export appearance. |
| D06 | P1 | BOM with balloons, cut list, hole/bend table, title properties and revision table. | Correct chosen assembly/configuration, human labels/quantity, no row drift after suppression, no overlaps/truncation on page. |
| D07 | P2 | Section/detail views and exploded-assembly sheet. | Actual supported view fidelity identified; no exact-section/hidden-line claim where implementation is approximate. |
| D08 | P1 | Duplicate/reorder sheets; save/reuse template; split long table over pages. | Sheet/annotation identities intact, title values correct, continuation headers/rows readable and complete. |
| D09 | P0 | Export PDF and drawing DXF, open/inspect files in suitable viewers. | All sheets and annotations present, same scales/labels, no missing linework/tables or stale output. Merely reporting a path is not an output pass. |
| D10 | P0 | Change source feature after drawing, preview repair/update, export again. | Current model and drawing match or export is blocked with clear diagnostic; stale dimensions cannot be silently published. |

## Git, Publish and exchange

Use only review projects and an explicitly chosen review destination. Keep new
hosted repositories private. Do not use the application source repository as a
sample model destination. Do not record passwords, tokens or authentication
screens. External forge effects must be stated in the report.

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| G01 | P1 | Open Git Connections/Repositories; browse or clone an accessible model with chosen connection. | Forge-neutral URL/local path workflow; author versus login clear; no secret in model source or recording. |
| G02 | P1 | Create local review repository and hosted private repository if already authorized/available. | Privacy default visible, intended destination confirmed, failure retains source. If hosted creation isn't exercised, mark Not run. |
| G03 | P0 | Save and commit project with unrelated local file and staged work kept separate. | Commit includes intended model/resources, user work preserved, version and branch clear. |
| G04 | P1 | Create/switch branch, compare saved revision, merge a benign model change. | Unsaved guard, useful source/geometry comparison, explicit merge and one validated outcome; no forced overwrite. |
| G05 | P1 | Fork a part, then assembly; edit fork and inspect source relation. | New identities and intended sharing scope clear, original remains unchanged, saved fork opens independently. |
| G06 | P1 | Transfer selected hole feature between branches with needed dependencies; handle a conflict. | Only selected closure imported, bindings shown, Keep Target/Use Source intelligible, Cancel/Undo restore original. |
| G07 | P1 | Generate interactive README preview, inspect image/link and spin model in viewer. | Preview corresponds to saved source; surrounding README prose retained; forge limitation/link behavior honestly documented. |
| G08 | P0 | First Publish with PDF/DXF/BOM/STEP to a review branch; inspect saved outputs and remote commit. | Correct repo/branch/folder, all selected files from same source, one commit, no unrelated files or credentials, clear completion state. |
| G09 | P1 | Reopen project, click Publish again after a small edit; change output choices then republish. | Per-project settings remembered; current outputs replaced, removed owned outputs cleaned, unrelated files preserved. |
| G10 | P1 | Publish project with no drawings, then with saved dimensioned sheets. | Automatic projection explicitly identified first time; later all intended saved sheets exported; no invented tolerances. |
| G11 | P0 | Safe disposable failed destination/upload and Retry saved upload; cancel before push. | Local result retained, exact commit retried, truthful uncertain outcome, newer remote never force-overwritten. Do not intentionally disrupt shared networks. |
| G12 | P1 | Normal Versions commit between Publish actions; edit remote unrelated file on review branch then republish. | Source branch/worktree stays intact; permitted histories integrate without losing unrelated file; managed model conflict demands review. |
| G13 | P1 | Publish optional STL/sketch DXF/flat DXF/cut list for matching projects; choose inapplicable optional output on copy. | Applicable outputs correct; inapplicable one stops entire publication with explanation rather than missing file disguised as success. |
| G14 | P0 | STEP export/reimport part and assembly with geometry/placements; inspect STL scale/orientation. | Geometry and units/instances retained for attempted cases; unsupported export fails clearly, original model unaffected. |
| G15 | P2 | Open supported external STEP solid/surface example with known dimensions; malformed/unsupported file recovery. | Format/scope/units clear, no silent partial import; exact unsupported carriers described. |

## Cross-cutting stress and usability checks

| ID | Priority | Manual workflow | Evidence required |
| --- | --- | --- | --- |
| X01 | P0 | Change a dimension rapidly, cancel while evaluating, then start a different edit. | No stale result applied; last committed source remains exact; pending/invalid display visibly distinguished. |
| X02 | P1 | Orbit and select safe display elements while large geometry/drawing/Publish work runs. | Navigation/progress/cancel responsive; no indefinite busy state. Record app time separately from automation latency. |
| X03 | P1 | Repeat normal edits across large nested assembly; inspect memory/responsiveness without inventing a benchmark. | No sustained unbounded rise or frozen controls in observed sequence; exact model scale and duration recorded. |
| X04 | P1 | Review every active task at narrow rail and enlarged UI scale. | No hidden OK/Cancel, clipped values, horizontal page spill, tiny-only hit targets or moving focused inputs. |
| X05 | P1 | Review long names, missing source, invalid unit/expression, invalid selection and unsupported geometry. | One plain next action plus expandable details; IDs/JSON do not dominate everyday workflow; no false success. |
| X06 | P1 | Save accepted projects, quit/reopen, inspect critical features/materials/mates/drawing sheets. | Native intent and resources persist; no result is counted solely from a transient preview. |
| X07 | P2 | Compare Classic and alternate theme on the same sketch, chamfer, assembly, drawing and Publish task. | Same commands/values/validation available in each; style differences do not remove essential behavior. |

## Recording plan

**Deferred by the user during the session.** Recording has stopped. The reviewer
will continue the hands-on review without making further videos. The plan below
is retained for future documentation work; these clips are no longer current
deliverables. See [recording-index.md](recording-index.md) for the earlier raw
capture's status.

Record short task clips alongside the longer review session. Raw feature footage
and defect evidence have different purposes; do not edit a failed attempt into a
successful demonstration. Each clip should have a manifest entry containing its
filename, app/build, theme, starting project, ending project, feature checklist
IDs, result, useful time range and known caveats. Keep raw captures out of ordinary
source commits unless intentionally selected and size reviewed.

| Clip | Suggested content | Reusable documentation outcome |
| --- | --- | --- |
| 01-workspace-and-theme | Empty start, planes, navigation, edge/face/sketch selection, compact rail, Classic/alternate/reset | First use and personalized workspace |
| 02-sketch-and-dimension | Rectangle, origin relation, tool-first and preselected Smart Dimension, Equal/Tangent, drag handles, Apply/Cancel | Sketch fundamentals |
| 03-machined-pedestal | Extrude, face sketch/boss, multiple holes/counterbores, selected pockets, rim fillet and chain chamfer | Everyday part modeling |
| 04-sheet-guard | Base, flanges/hem, supported cut, Formed/Flat, bend output | Sheet-metal workflow |
| 05-structural-frame | Pick sketch paths, profile/location, mixed groups, trim/cap/gusset, visible supported weld | Weldment workflow |
| 06-assembly | Insert/ground, selection versus edit, mates, pattern by edge, nested tree, isolate and shared edit | Assembly fundamentals |
| 07-finishes-and-libraries | Missing-pack fallback/download, separate material/finish, grayscale grain and lighting | Appearance and content libraries |
| 08-drawing | Standard views, associative dimensions/callouts, tolerance, BOM/cut-list/balloons, PDF | Manufacturing documentation |
| 09-versions-and-update | Branch, fork, pinned component update, selected-feature transfer and preview | Versioned design collaboration |
| 10-publish | First setup, output choices, Publish/progress, inspect files and saved choices | One-action publication |
| 11-review-turntable | Clean finished fixture, exploded state, close-ups of weld/finish/details | Feature overview footage |

Use consistent framing and readable text. Pause briefly after each accepted
action and before closing its result. Capture the visible cursor and whole
relevant window; exclude credentials and unrelated private windows. Preserve
short defect clips with exact failure messages and recovery. Label silent raw
recordings as silent; do not imply narration or edited tutorials exist.

## Prior dogfood regression map

These are **historical observations**, not current failures. Implementation
documents say fixes were added, but they explicitly did not repeat interactive
acceptance. Run the named reproducer rather than declaring a fix accepted from
source review.

| Earlier finding | Current review rows | Source |
| --- | --- | --- |
| Formed-sheet cut ancestry exhaustion | F02–F04 | [Three-part review](../usability-review-2026-10-12.md), R1 |
| Counterbore/pocket open cap arrangement | M04–M05 | Three-part review, R2 |
| Adjoining planar chamfer rejection | M06 | Three-part review, R3 |
| Rim segmentation, preview feedback, crowded sketch rail and framing | R04–R08, K03–K08, M01/M06 | Three-part review, R4–R9 |
| Misleading body count, UUID cut list, numeric noise, overlays/finish/save | R13, F06/F10, L06–L09 | Three-part review, R10–R16 |
| Inaccessible chamfer manager | M06, X02–X04 | [Conveyor review](../conveyor-project-usability-review-2026-10-13.md), P1-01 |
| Large drawing inaccessible | D01/D08–D10, X02–X03 | Conveyor review, P1-02 |
| Wrong occurrence seed, ghost picks, flat assembly tree, buried task controls | R06–R08, A01/A05/A06/A08 | Conveyor review, P1-03–P1-06 |
| Existing-bore counterbore, direction from line/axis/edge | M05/M11, A06 | Conveyor review, P1-07–P1-08 |
| Moving fields, clipped buttons, repeated diagnostics, seed-vs-constraint confusion | R08, K03/K06/K07, X04–X05 | Conveyor review, P2-01–P2-09 |
| Formed/Flat history clutter, repeated-body highlight cost, striped grain | F04, L07–L08, X02–X03 | Conveyor review, P2-10–P2-12 |

## Static scope and distribution risks

These come from documents, not this review's desktop observations. Keep them in
the final report as documented limits unless a current attempt adds runtime
evidence. The complete [mechanical coverage inventory](../mechanical-cad-coverage.md)
still calls every major family Partial/Missing/Audit; this session cannot certify
parity or all supported hardware/platforms.

- **Starter-pack access:** [Content Libraries](../content-libraries.md) says all
  five initial sources are private and require access. A general release needs
  an explicit access/distribution decision; do not make repositories public as
  part of review. Verify fallback wording for a user without access.
- **Geometry breadth:** general freeform/tangent Boolean completeness, arbitrary
  blend junctions, variable/full-round fillets, broad freeform offsets, loft guides
  and some STEP boundary classes remain limited. Unsupported must mean a clear
  preserved-document outcome, not false geometry.
- **Drawing fidelity:** exact hidden-line removal, exact/general sections, broad
  auxiliary views and full standards certification remain gaps. Evaluate the
  actual exported drawing; do not market a wireframe or approximate section as
  complete production validation.
- **Manufacturing breadth:** imported-body sheet conversion/unfolding,
  partial-width/miter/swept flanges, forming tools, general weld paths/grooves,
  production threads and broader mold tooling remain outside current support.
- **Assembly breadth:** flexible subassemblies, unrestricted mechanical winding,
  general collision exclusion, assembly cuts/hole series and large-assembly
  lightweight modes remain gaps.
- **Presentation completeness:** some editor families still need independent
  per-control semantic migration. A radically different theme must be exercised
  on real tasks; visual recoloring alone does not establish full styleability.
- **Physical input/platforms:** one Mac/automation session cannot establish real
  finger comfort, Windows/Linux support, external trackpads, keyboard-only or
  assistive-technology acceptance. Name these Not run unless physically checked.
- **Publication:** one-action Publish has compilation/lint/package evidence only
  before this review. Remote changed-file safety, retry truthfulness and generated
  output freshness are release-critical review targets, not established passes.
  During this review, the first local-bare-repository Publish produced and pushed
  the pedestal source/PDF/DXF/BOM/STEP; the bounded result is recorded in
  [report.md](report.md). It does not establish hosted authentication, conflict or
  retry behavior.

## Result ledger template

Maintain the observed results in a separate report so the checklist remains an
honest plan. For each finding use:

| Field | Required entry |
| --- | --- |
| Finding | Stable ID, short plain title and P0/P1/P2 |
| Checklist | Relevant row IDs and exact attempted modes |
| Build/context | App build, project path, part/occurrence/configuration, theme |
| Reproduction | Starting checkpoint and smallest repeatable click/value sequence |
| Expected / observed | Separate statements; exact visible message when relevant |
| Recovery | Cancel/Undo/reopen outcome and whether source was preserved |
| Evidence | Screenshot/video filename plus time; saved successful/reproducer project |
| Classification | Product issue, automation limitation, known documented limit, or design incomplete |
| Recommendation | Concrete user-facing change; do not claim implementation is complete |

End the report with actual delivered project/output/video paths, a P0/P1 decision
list, supported workflows demonstrated, and all blocked/unrun areas. Avoid a
completion percentage or a blanket “release ready” from a subset of rows.
