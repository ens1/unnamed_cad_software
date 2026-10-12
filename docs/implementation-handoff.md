# Implementation handoff — 2026-10-10

The user resumed work after enabling Ultra; the earlier stop request is revoked. The latest target is the mainstream mechanical CAD capability set shared by SolidWorks, NX, Onshape and Creo, replacing full vendor parity. Completion is unclaimed. This handoff records saved source capabilities and their actual geometry/solver bounds. The continuation adds ribs, direct face edits, body filtering, reference curves, curve/sketch/table patterns, grouped dimensions, equation curves, tangent/bisector planes and supported STEP surface import. This continuation also adds exact tap-drill holes with explicit cosmetic thread intent/callouts, directed ruled and bounded matching mid-surfaces, drawing sheet formats/templates/linked title cells, saved paper view layout, and root-assembly exploded views shared by viewport and drawings. No code tests, assertion probes, manual smoke tests, export exercises or new interactive acceptance have been performed.

## Integrated source

- Sketch Trim/Extend/Split with preserved analytic/rational geometry; arbitrary-axis symmetry; bounded point-on-curve, tangency and curvature contact on analytic line/arc/circle/ellipse/Bezier/positive clamped NURBS carriers. Saved normalized fixed/free contact parameters use exact derivatives and require regular tangents/C2 curvature locations. Driven/reference dimensions preserve their authored expression, measure the current geometry and add no solve equation. Arc length/included-angle and ellipse dimensions join the existing coordinate/length/angle modes. Rank/DOF and residual diagnostics are published; ambiguous or nonconvergent definitions block Apply.
- Editable interpolating and control-point NURBS splines with fit/control handles, degree, knots, positive weights, parameterization and expressions. Exact rho conics, parabolas, partial/full rational ellipses and true circular arc slots retain editable source intent.
- Sketch editing includes bounded line–line/line–arc/arc–arc tangent fillets, straight-corner chamfers, exact line/arc/circle offsets and algebraically proven straight/circular Bezier/NURBS or degree-one polyline offsets. Repair joins only mutually unique independent near endpoints through explicit coincidence and can remove exactly duplicate relations. Stretch targets explicit independent controls; Replace retains the target member identity and compatible references. Dynamic mirror enrolls newly drawn source members; generated pattern instances support suppression and additional affine placement. Suppressed outputs and their relations are excluded from profile construction/solve without discarding their saved identities.
- Sketch Text retains pinned static local TrueType/OpenType/collection font bytes and application-owned exact polynomial outlines. Em-height, offsets, rotation, tracking, alignment, multiline layout, construction state and a draggable anchor are editable. Compatible glyph/member IDs survive edits; retiring referenced contours fails explicitly. Bounds are 32 MiB per font, 1024 characters and 32768 outline segments. Font/render objects are not saved as authoritative geometry.
- Stable Selected Contours on Extrude/Revolve/Loft/Sweep retain current native material regions or supported thin chains and contained holes. Multiple boundary-member picks resolve deduplicated regions, including picks on contained hole boundaries. Explicit selected order gives Loft correspondence; new connectors anchor native regions, while legacy ordinal connectors must be explicitly anchored before selected-region use. Associative point sources add model/datum extrusion starts.
- Sketch blocks retain same-sketch native source definitions/internal relations, stable instance output maps and parametric positive uniform placement. Prepared local solves are reused across equivalent instances and outer solver residuals. Independent anchors and optional literal angle variables supply rigid instance DOFs. Docked source/instance editing, local control/anchor dragging, construction/suppression and exact Explode use the shared sketch draft.
- Extrusion retains sketch-normal or explicit/associative axis direction, sketch/planar-face/native sketch-vertex start and offset, independent opposite end conditions, Mid-plane and Through All Both. Ends include Blind, Through All, planar Up to Face/Offset from Face, stable sketch-point Up to Vertex and proven unique fully covering forward Up to Next/Body. Curved termination uses a full four-sided separable positive-rational graph patch over a rectangular normal section. Thin line/arc/circle sections support one-side/two-side/midplane thickness and flat ends; drafted extrusion requires a straight closed profile and normal direction. New/join/cut/intersect operate on explicit feature-output scope.
- Native revolutions add reverse, midplane, independent second angle, angular start and thin sections, with total extent no greater than one full turn. Actual rational/revolution carriers and shared partial caps replace separate approximated end geometry. All nonzero starts are applied; only an exact full-turn extent closes the revolution. A half-plane control-hull proof, one connected stationary axis boundary chain and native near-axis/pole guards bound supported singularities. Wall/station/endpoint ancestry names generated topology.
- Solid lofts retain ordered parallel planar sections, compatible holes/multiple regions, native boundary-vertex connectors, exact basis refinement/compatible normalized rational weights and ruled or cubic endpoint normal/reference-axis influences. Affine, convex nested-polygon or concentric-circle proofs reject folds/intersections; endpoint controls must advance axially. Solid sweeps retain an explicit member or connected open sketch chain, persistent chain-start vertex, native parameter trim/reversal, source/path-start placement and roll. Fixed sections support monotonic positive-rational paths and exact rational half-angle twist. Tangent-following supports collinear or native circular paths with radial clearance and zero circular twist. Both provide supported thin sections and new/join/cut/intersect body scope; their new walls/edges retain member and incident-wall ancestry.
- Application-owned parametric datum planes/axes/points and coordinate systems drive sketches, revolve/pattern axes and mirror/draft planes. Coordinate systems use an associative point origin and two explicit/associative directions, independent output-axis assignment and reversal, with primary-direction preservation and a projected secondary direction. Nonfinite, degenerate, parallel, nonorthonormal or left-handed frames fail. Their origin, three axes and XY/XZ/YZ planes are independently semantic, displayed and selectable; coordinate planes host sketches, and complete frames retain assembly twist orientation. Configurations/history and complete occurrence paths apply to all consumers.
- Associative model-edge point sources retain the owning body, construction reference and scalar 0..1 expression. Native stored curve orientation defines endpoints and parameter midpoint independently of incident faces; finite intervals, endpoint consistency and construction provenance are required. Shared docked point selectors, selected-edge actions and mate start/midpoint/end choices expose the same source/parameter fields. Datum/coordinate/feature-center and mate consumers resolve these exact native carriers; midpoint is not an arc-length claim.
- Separate exact surface bodies: planar, extrude, revolve, ruled loft/sweep, restricted planar offset/thicken, orientation reverse, and restricted matching-rational-boundary knit with closed oriented shell conversion. Mesh/history/STEP paths preserve actual solid/surface kind and output ownership.
- Shell supports inward/outward per-face thickness on convex supporting-plane stock, proven concave straight/circular prisms with through holes and cap/contour thickness overrides, and inward shelling of contained disjoint cylindrical blind/through pockets. Supported openings share actual rim topology. Retained faces/edges inherit their source roles; new offset walls, cap/rim partitions and expanded pocket boundaries use normalized source closure and constructor intent, without centroid/signature naming.
- Native simultaneous constant-radius planar-stock rounds with cylindrical walls, cap trims, supported two-edge mitres and three-edge spherical corners. Equal/two-distance/distance-angle chamfers reconstruct selected original setbacks with shared chain/corner topology, merged coplanar split setbacks and explicit ancestry; optional associative distance-1 face remains supported.
- Associative standard mates retain coincidence/planar, parallel/perpendicular, concentric/rotation lock, distance, angle and rigid lock. Advanced relationships include Width, Symmetry, Limit Distance/Angle, Profile Center, Linear Coupler and Path. Mechanical relationships include Hinge, Gear, Rack and Pinion, Screw, Universal Joint, Slot and Cam / Roller. Exact supported frames/carriers resolve configured/nested geometry and complete coordinate frames before bottom-up rigid-subassembly solving; in-context geometry rebuilds afterwards. Feedback cycles are rejected. Mechanical zero frames are captured from current validated placements. Per-mate residual/rank/redundancy, inequality counts, solved carrier parameters and component nullspace/DOF diagnostics are application-owned evaluation data. Make Unique and component copies remap only affected full occurrence/source paths, including auxiliary/support references and captured frames.
- Associative drawing dimensions, explicit units/precision, tolerances, datums/GD&T/material modifiers, leaders and center geometry. Shared paper vectors drive main-viewport preview, PDF and drawing DXF; standard DXF views retain supported exact projected rational curves. Configurations, solved placements and consuming topology branches are resolved. Drawing reference validity participates in worker acceptance.
- Cached f64 boundary-tessellation area, volume, centroids, material-density mass and inertia by body/component/assembly, with placement/parallel-axis aggregation and finite numeric guards. These are tessellated estimates, not exact B-rep/error certificates.
- Shared output-consumption reconciliation when feature inputs change, preserving independent branches and restoring removed inputs where appropriate. Feature and mate source editors cannot apply invalid/rejected/unsynchronized text ahead of the evaluated draft.
- Dependency-preserving authored feature reorder uses a native destination-index command and feature-tree/semantic Move Up/Move Down actions. Source IDs and references remain intact; invalid source-before-consumer order fails. The common worker draft previews the reorder, current validation gates one undoable Apply, and Cancel restores its checkpoint. Order is shared across configurations; cached History remains a separate display operation.

Final source review corrected integration defects in arc-fillet library identity remapping, block editor restoration after canceled/undone drags, construction toggles on overridden block instances, and contextual semantic feature actions. A whole-instance construction override still applies to the entire block instance; members without an override edit the shared block source. Implicit intersection edge points use fallible projection and supporting-surface residual checks, preserving construction endpoints only after consistency checks. Invalid projection and known ambiguous Boolean carrier ancestry fail explicitly.

The current continuation also integrates:

- Sketch-driven Rib with connected open contours, parallel/normal fill, supported bounded planar stock/face attachment, thickness placement and source ancestry. Direct Move/Offset/Delete-Heal/Replace Face reconstructs convex planar supporting halfspaces. Keep/Delete Bodies uses explicit owning-output/construction-face anchors and exact whole-solid filtering, including a valid empty result. Suppression restores every compatible consumed branch and rejects conflicting ancestry/mixed output kinds. See [ribs](ribs.md), [direct editing](direct-edit.md) and [body filters](body-filters.md).
- Composite Curve, Curve Through Points and Helix/Spiral retain ordered native sources, linear/natural-cubic point interpolation or analytic constant-pitch/tapered winding. Rational zero-pitch circles remain usable by supported Sweep and Convert Entities paths. Display curves use fallible native evaluation and tolerance-driven subdivision; generic parameter/intersection chord error is estimated rather than globally certified. Curve Pattern has native parameter or proven line/circle/helix/spiral distance spacing, range/reversal, fixed/tangent frames, source retention and stable skips. Sketch/Table Driven Patterns retain explicit point sources or coordinate expressions and stable authored row IDs. See [reference curves](reference-curves.md), [curve patterns](curve-patterns.md) and [placement patterns](placement-patterns.md).
- Ordinate/Baseline/Chain sketch dimension groups retain X/Y datum/order/constraint bindings and draggable annotations. Exact polynomial Equation Curves retain coefficient/range/angle expressions, an independent anchor and stable derived Bezier controls with explicit reference-retirement guards. Both use shared sketch drafts, current evaluation, Apply/Cancel and undo. See [dimension groups](sketch-dimensions.md) and [equation curves](sketch-equation-curves.md).
- Angular bisector planes retain both signed-distance solutions. Tangent planes retain native face ancestry and normalized parameters or associative point anchors on proven bounded rational patches and cylindrical/conical carriers; regular derivatives/domain proofs reject unsupported trims or inverse ambiguity. See [reference geometry](reference-geometry.md).
- STEP surface-model imports retain pinned resources, actual model/shell/face/edge entity identity and loss-rejecting native conversion. Mixed supported solid/surface definitions share their imported component/placement structure. An exact AST cache adapter recognizes manifold/bounded-surface representation subtypes and retains all DATA sections, IDs and product links; unsupported geometry-set/mapped-item contents remain rejected. Strict representation/unit/placement guards reject unsupported geometry rather than silently dropping it; broader carriers, ambiguous periodic trim intent and translator breadth remain work.
- Native designed holes from explicit stable position-sketch points: simple, counterbore and countersink; blind flat-bottom/through-all, reverse entry and start offset. Selected planar-face commands start a Point sketch on that face. Deferred hole/fabrication actions bind to the document/part/body context, await current validation, and clear on Cancel. Exact shared circular stations cut multiple disjoint holes in proven convex straight planar stock; other supported stock uses the validated Boolean path. Each generic drill must have a changed carrier surviving the final cut. Configured drawing callouts resolve authored bore/head dimensions and actual exact circle/axis/center correspondence.
- Positive uniform/XYZ body scale about an associative center, optional original retention, two linear pattern directions, skipped positions, circular total/instance angle and rotated/fixed orientation. Transformed carriers propagate provenance. Compact structured pattern/hole-position/intersection/construction roles retain source topology and position identities without exponential nested escaping. Generic Boolean intersection curves with known constructed carriers use surface-carrier ancestry; ambiguous authored carrier matches and disconnected edges sharing the same carrier pair fail instead of receiving centroid names. Unknown/legacy carriers retain compatibility fallbacks. Library import/Make Unique recursively remap engineering IDs while preserving human text, paths, hints and unaffected shared occurrence references, including saved arc-fillet source/through maps.
- Named/derived part/assembly configurations with inherited parameters, feature/sketch/component/pattern/mate suppression, material/properties/BOM and child configuration overrides. Docked editors cover variants, equations and local JSON/CSV design tables. Linear/circular component patterns reserve occurrence/copied-mate identities across count reduction/restoration. Simultaneous subassembly variants have distinct geometry, solved placements, STEP DAG nodes and manufacturing scope.
- Native sheet base flanges/tabs with holes and positive/reverse/midplane thickness; full supporting straight-edge flanges with signed angles through 90 degrees, positive inside radius and parametric K factor. Formed/flat stock shares source and exact bend topology. Current exact flat cutting curves/bend lines feed DXF and bend tables; export selects one blank per placed part/configuration rather than orphan definitions and rejects nonfinite coordinates.
- Weldments with multiple straight planar path/section groups, locating point/roll, solid/hollow sections, independent butt/two-member miter ends, shortening/extension, associative planar trim/extend, terminal caps and sketch gussets. Trim scope binds to inherited groups. Evaluated centerline/cut-plane geometry, material and configured quantities feed cut-list CSV.
- Exact four-boundary rational/polynomial Euclidean Coons fills with bounded algebra and global rational projection certificates; planar sketch trim; finite rational U/V trim retaining original carriers for Untrim; selected/all straight-edge extension on independent convex planar faces; and coplanar-shell offset/Thicken with shared topology/holes. Native extrusion rejects implicit intersection/parameter boundaries explicitly.
- Mold Draft Analysis reports exact planar normals against an associative pull axis, leaving curved faces unassessed. Exact selected-edge parting strips, planar-loop shut-offs, positive associative Shrink and sketch-stock/parallel-plane tooling core/cavity split retain saved source intent. Derived reports are analysis DTOs rather than manufactured outputs.
- Selected face/circular-edge/solved-sketch-circle axes resolve exact analytic revolution or proven rational ruled cylinder/conical-frustum carriers with degree 2–32 Bernstein identities. Shears, unequal radial scaling, implicit guides, short ill-conditioned arcs and rational apex sections fail instead of fitting a displayed mesh.
- BOM/revision/bend/general drawing tables with editable paper placement, widths, row/text sizes and space reservation. Stable BOM numbers reserve part/configuration identities across suppression; associative balloons follow those rows and quantities. Shared preview/PDF/drawing-DXF vectors and current validation include tables and balloons. Flat-pattern DXF and evaluated cut-list CSV are grouped File actions; immutable releases include current fabrication artifacts when present.

Native schema remains 1; additive defaults omit empty/None values so existing source hashes remain stable. The saved contract constants are:

| Contract | Version |
| --- | --- |
| Sketch editing / transformation / text | 4 / 2 / 1 |
| Extrusion / revolution / solid sections / wall options | 3 / 1 / 1 / 1 |
| Reference geometry / coordinate systems | 5 / 1 |
| Assembly geometry | 4 |
| Selection actions / drawing | 3 / 5 |
| Body editing / surface intent | 2 / 3 |
| Configuration / component pattern / drilled holes | 4 / 1 / 2 |
| Sheet metal / weldment / mold / drawing tables / inspection | 1 / 1 / 1 / 2 / 1 |
| Feature order / contour selection / sketch blocks / sketch handles | 2 / 2 / 2 / 2 |
| Derived sketch section topology | 1 |
| Rib / direct edit / body filter / reference curves | 1 / 1 / 1 / 1 |
| Dimension groups / equation curves / curve pattern / placement pattern | 1 / 1 / 1 / 1 |
| Split / assembly inspection / exploded views / drawing sheet formats | 1 / 1 / 1 / 1 |
| Portable block resources / configured dimension fields / drawing symbols | 1 / 2 / 1 |

Core solver source reports `tcad-native-lm/14`. Adapter and desktop source constants use `monstertruck/0.4.1+fc2306852a46;adapter/31;solver/tcad-native-lm/14`. Transient sketch diagnostics and assembly movement each use version 1. Internal boxing retains native JSON shapes. The kernel directly declares the existing workspace `serde` dependency for bounded report serialization; the desktop now directly declares the already-pinned `winit` 0.30.13 for primary-window gesture phases. No new third-party package or dependency version is introduced, and existing dependency notices cover both.

The preceding interaction correction adds [exact face-attached boss joins](boss-joins.md), [two-pick Smart Dimension](sketch-smart-dimension.md), and saved [navigation preference contract version 1](navigation.md). Contact joins remove native opposing caps and retain real shared edge/face ancestry for supported inset and matching-outline contacts, including separated repeated bosses. Smart Dimension supports tool-first picking or Command/Shift preselection, signed-side center-to-line gaps, current owned straight model-edge construction conversions, live values and a local dimension checkpoint including promoted parameters. New conversions retain construction parameter orientation; legacy incident-face ordering remains explicitly readable. The trackpad adapter reverses the default scroll-orbit directions and adds cursor-anchor pinch, native roll/smart zoom, saved sensitivities/direction switches, precision modifiers and gesture ownership/cancellation. Geometry and hardware limits remain recorded in the capability documentation.

## Previous package verification

Before this source expansion, `cargo fmt --all` and `cargo clippy --workspace --locked -- -D warnings` passed, and `scripts/package-macos.sh` completed its workspace build and package. That package used distinct executable names `TCAD`, `tcad-worker`, and `tcad-cli`; the existing dependency future-incompatibility notice concerned `proc-macro-error2` 2.0.1. Those historical results do not verify the latest source. No native application or CLI CAD operation was run as acceptance for that package.

## Previous integration verification

The adapter 16 / solver 11 interaction corrections passed final `cargo fmt --all`, `cargo clippy --workspace --locked -- -D warnings` and `scripts/package-macos.sh` on 2026-10-10. The refreshed app is `dist/TCAD.app`, using distinct executables `TCAD`, `tcad-worker` and `tcad-cli`. The existing future-incompatibility notice remains `proc-macro-error2` 2.0.1. Compilation and linting establish source integration only; interactive behavior, geometry robustness and usability remain with the user. No tests, probes or UI acceptance exercises have been performed. Source review also corrected construction-edge orientation, secondary-click pair retention, pending edge highlighting, camera-after-navigation scheduling and native gesture segment ownership. Touchscreen camera navigation remains explicitly unsupported; native macOS trackpad gestures are supported by the platform adapter.

## Previous loft and selection integration

Adapter 17 retains saved source contracts and advances the geometry fingerprint so earlier derived loft caches cannot mask corrected native correspondence. Exact contour refinement, rational carriers, surface boundary sharing and selected geometry presentation are integrated. [Loft geometry](loft-geometry.md) records native correspondence, proof limits and old-reference repair requirements. Final `cargo fmt --all`, `cargo clippy --workspace --locked -- -D warnings` and `scripts/package-macos.sh` passed on 2026-10-10 for adapter 17 / solver 11. The package is `dist/TCAD.app`, with distinct executable names `TCAD`, `tcad-worker` and `tcad-cli`. No tests, probes, CLI CAD operations or UI/manual smoke exercises were written or run. Interactive acceptance remains with the user. The existing dependency future-incompatibility notice concerns `proc-macro-error2` 2.0.1.

## Previous complex-loft correction

Adapter 18 adds direct degree-one uncontrolled loft carriers, native support
seams and bounded geometric perimeter allocation for nonradial correspondence.
Actual signed native-area enclosures establish winding while preserving the
first contour's native sense, including hole and negative-axis solid sections.
Surface-only shared-column chart refinement and projected-Jacobian principal
minor guards admit valid curved/sheared patches that the previous fixed SPD
guard could not certify. Differentiability gates the principal-minor theorem;
nonsmooth knot joins retain the stronger SPD guard. Arithmetic majorants and
fixed-frame proof subdivision remain bounded; uncertain carriers are diagnosed
with section-pair/strip context rather than described as proven folds. Source
schema and saved intent remain unchanged. Derived caches are invalidated and
additional face splitting can require reference repair. No runtime geometric
acceptance has been performed; verification status is recorded below.

Final `cargo fmt --all`, `cargo clippy --workspace --locked -- -D warnings` and
`scripts/package-macos.sh` passed on 2026-10-11 for adapter 18 / solver 11.
The refreshed package is `dist/TCAD.app`, retaining distinct `TCAD`,
`tcad-worker` and `tcad-cli` executables. The dependency future-incompatibility
notice remains `proc-macro-error2` 2.0.1. No tests, assertion probes, CLI CAD
exercises or manual/UI smoke tests were written or run. Saved recovery files
were read only to identify authored section geometry; their loft was not
evaluated as acceptance.

## Product scope and remaining work

The shared mainstream mechanical CAD baseline remains the target. No product-completion claim follows from this integration. See [coverage](mechanical-cad-coverage.md), README limits, [configurations](configurations.md), [references](reference-geometry.md), [mates](assembly-mates.md), [revolutions](revolutions.md) and [Shell provenance](shell-provenance.md). The remaining engineering includes:

- Sketching: general ellipse/spline parallel offsets and spline fillets; nonpolynomial/implicit equation curves, periodic/surface sketch splines and spatial sketches; oblique/radial/angular grouped dimensions; broad repair/intersection tools. Repair does not merge ambiguous endpoints or resolve driven-control conflicts. Destructive edits require detaching authored spline/conic/slot intent. Transform bounds are 2–256 copies/4096 entries; incompatible patterned control topology requires explicit repair/detach. Text lacks kerning, ligatures, combining/bidi/context shaping, variable/color-only fonts, path layout and automatic contour healing. Sketch blocks support same-sketch associative native definitions and positive uniform instances; portable files add cross-document insertion and explicit same-resource updates. Nested blocks, library indexing/browser, variable scale solve and a full local drawing-tool suite remain gaps.
- Extrusion: curved ends require complete four-sided separable rational graph carriers over rectangular normal profiles, with no old trims/holes and zero normal offset or an explicitly authored exact offset surface. Up to Next/Body requires one unique complete nearest forward cap; piecewise/crossing envelopes remain unsupported. Thin sections and draft retain their recorded carrier/plane bounds. Broad intersecting old-void edits and general Boolean robustness remain work.
- Revolve/loft/sweep: revolution pole/axis-crossing reconstruction beyond one connected stationary axis chain; nonparallel lofts, arbitrary guides, curvature/adjacent-surface and general boundary continuity; general spline moving frames, circular-path twist, guide/tool-body sweeps and self-intersection trimming. Solid loft connectors must select actual boundary vertices; rational correspondence and strict axial separation must be proved. Sweep fractions use the native parameter domain with equal member spans, not arc-length percentage or constant-pitch twist. Surface Loft now shares exact unequal-edge refinement and rational skin correspondence, with ordered sections, per-strip/patch guards and authored boundary ancestry. Surface Loft/Sweep remain ruled/fixed-polyline workflows with recorded limits; global self-intersection certification, guides and smooth continuity remain work.
- Shell/Draft/direct editing: unrestricted curved/concave offsets, interacting pocket/wall/floor envelopes, arbitrary side openings and full multi-thickness prism contours; broad curved/concave direct editing, parting-line draft and fillet/chamfer scope. Convex supporting-plane Shell permits per-face thickness, while each prismatic contour requires one supported wall offset. Pocket shelling is inward with contained separated cylindrical voids. Neutral-plane Draft remains convex, straight, planar and unsplit. Chamfer clipping rejects intersected curved/pocket boundaries and unsupported caps. Constant-radius edge rounds retain their documented convex straight/planar stock and supported trivalent-corner envelope; general curved/concave blends, variable/face/full-round options and broader corner networks remain work. Hole standards, modeled/external thread teeth and drill-tip geometry remain absent; native tap bores and explicit cosmetic thread intent are implemented.
- Surfaces: curved normal offsets/thickening, mutual/curved trim and extension, general rational fills, gap healing and smooth boundary continuity. Knit requires matching positive-weight rational boundaries and 1e-10–1e-6 mm sew tolerance; Form Solid validates closed oriented topology without certifying global self-intersections or outward volume. Offset/Thicken currently accepts consistently oriented coplanar face shells. Implicit intersection/parameter boundaries remain unsupported in native extrusion.
- References/assemblies: general trimmed/periodic/singular tangent planes, independent B-rep vertex/arc-length point modes, automatic mass-center origins, projected/on-surface curves and analytic helical solid sweeps; curved/spherical mating, 3D/composite paths, mate-aware dragging, flexible scopes, assembly features, certified general minimum clearance/contact classification and sparse large-assembly performance. Profile Center covers one planar material region and uses estimated rational-integral accuracy; Cam is local planar profile contact and Slot needs a line/circle center path. Mechanical motion uses principal ±180-degree coordinates, with multi-turn winding still missing. DOF diagnostics are local infinitesimal freedom. Conflicting occurrence equations, external mates on generated copies and reflection component patterns fail clearly.
- Patterns: feature replay, face/component scope for new pattern modes, fill/variable patterns, general spline exact-distance spacing and wider orientation/path continuity. Copies remain rigid whole native body sets with stable constructor identities.
- Fabrication/drawings/exchange: general sheet recognition/unfolding, corner relief/hems/jogs and swept/miter flanges; curved/spatial weldments and separate cap/gusset cut-list rows; global mold release/undercut certification, side cores and curved parting/shut-off networks; exact hidden-line removal, arbitrary sections and full standards-specific GD&T; broader neutral/native translators, geometric healing and arbitrary surface trim import. Sheet developed envelopes must remain disjoint. Weldment lengths are centerline lengths. Inspection uses boundary-tessellation estimates, without exact B-rep integrals/error certificates. Topology splits/merges and ambiguous construction ancestry can still require reselection.
- Feature-history editing: source reorder has dependency-preserving commands and Up/Down controls; tree drag/drop, editable rollback/freeze and automatic dependency retargeting remain gaps.

Unsupported or ambiguous modeling geometry and invalid source references block modeling confirmation without replacing valid committed definitions with substitute geometry. Unsupported inspection pairs remain explicit report diagnostics alongside an otherwise valid saved inspection definition. Hardware gesture fidelity, representative-part robustness, performance and interactive usability remain unverified.

Future work must retain application-owned f64 authority, explicit units/tolerances, stable/provenance references, persistent workers and stale-result rejection, current-result Apply, checkpoint Cancel, one model viewport, sketch-first creation and user-defined presentation. Compile/format/lint only; interactive acceptance belongs to the user. The existing `models/two-inch-cube/project.json` is from the earlier user-requested UI exercise and is not new acceptance evidence.


Earlier additional contracts: Hole **2**, Surface **3**, Drawing **4**, sheet format/template **1**, Exploded Views **1**. Saved root assembly explode stages never change mechanical placements, engineering properties or STEP/STL. Full occurrence targets are bounded and configured; inactive targets retain source identity. The latest source review repaired inactive-config Apply, stale picked display coordinates on stage/worker changes, unchanged-editor OK, and target/cache/display resource bounds. Drawing source scratch gates Apply and shared exports, and Cancel/Apply/Undo/Redo/project replacement clears obsolete scratch. Cosmetic threads represent exact tap bores plus explicit intent/annotations; modeled teeth/external threads and standards tables remain work. Ruled/mid-surface definitions retain source geometry and prove supported exact bounded carriers rather than fitting missing surfaces. See [exploded views](exploded-views.md), [holes and threads](holes-and-threads.md) and updated surface/drawing documentation.

The additional integration adds [Split / Cut With Plane](split-bodies.md) contract 1, [Assembly Inspection](assembly-inspection.md) contract 1 and [Drawing Tables](drawing-hole-cut-tables.md) contract 2. Split retains exact native stock-side ancestry with finite containment bounds. Inspection keeps native-common/tangency/unsupported statuses distinct from approximate boundary distances; selected A/B occurrence boxes and boundary witnesses are presentation only. Hole and native member tables reserve source row numbers across order/configuration/suppression changes and use shared manufacturing vectors. Source row admission is bounded before allocation; default native member groups reserve every possible authored path member. All source changes use the existing draft/validation/Cancel/undo path.

The adapter 17 selection correction shares the viewport selection color across members, handles, whole sketches, edges and faces. A selected face receives a depth-tested translucent display overlay and native boundary outlines. Picking and feedback share a displayed/input-checkpoint evaluation and require matching native body, producer, topology and complete occurrence path. Root part selections no longer treat the part definition as an assembly occurrence; old saved aliases remain readable. Viewport sketch clicks preserve full occurrence paths before double-click editing opens, and History navigation clears transient scope. These are source integration findings, not UI acceptance results.

The 2026-10-11 continuation adds configuration contract 3 / dimension field descriptor 2 (typed feature expressions, identified-record selectors, inherited JSON/CSV columns), Drawing 5 / symbol intent 1 (bounded surface-finish and weld conventions), and Sketch Blocks 2 / portable resources 1 (detached exact source files, explicit updates, provenance-preserving remap). Fill now retains exact variable-weight rational boundaries and source-derived topology with degree-16 proof support. The solver skips empty-equation Jacobian sampling. Compilation/formatting/strict workspace lint and macOS packaging establish integration only; no code tests or interactive acceptance were performed. See the updated coverage rows and linked capability bounds.


## Current edge-feature correction

Adapter 20 replaces the generic fillet path, which could skip failed selected
edges and still report success, with native exact constant-radius construction.
All selected eligible stock edges are reconstructed together; terminal cap,
supported two-edge mitre and three-edge spherical corner boundaries share actual
native topology. Chamfer uses oriented unmatched clipped-shell edges to close
bevels, merging coplanar split setbacks and preserving proven disjoint old voids.
Both operations resolve the complete edge set once, partition it by unique
input-solid ownership and retain untouched solids. Generated/trimmed walls and
edges record explicit source ancestry through `ShellHistory`.

Desktop edge selection accumulates instead of replacing the list, supports
modified-click preselection/toggling, highlights the complete set and exposes
Add/Remove/Clear through stable semantic commands. Existing edits pick cached
input-stage geometry. Full occurrence-path scope prevents a repeated shared
subassembly leaf from collecting edges in another occurrence. Saved native
source remains schema 1 / edge-feature intent 1; the changed adapter fingerprint
invalidates old derived geometry. Older generic blend-output references can
require explicit reselection. See [fillets](edge-fillets.md) and
[chamfers](edge-chamfers.md) for actual geometric scope and remaining gaps.


Final adapter 20 / solver 12 integration passed `cargo fmt --all`,
`cargo clippy --workspace --locked -- -D warnings`, `git diff --check` and
`scripts/package-macos.sh` on 2026-10-11. The refreshed package is
`dist/TCAD.app`, with distinct `TCAD`, `tcad-worker` and `tcad-cli` executables.
The existing dependency future-incompatibility notice remains
`proc-macro-error2` 2.0.1. No tests, assertion probes, CLI CAD exercises or
manual/UI smoke tests were written or run; geometric and interactive acceptance
remain with the user. The canonical GitHub repository remains private.

## Visual finishes and illumination (2026-10-11)

The independent `cad-core::appearance` contract v1 owns finish/map/mapping/lighting definitions and assignment commands. The desktop uses embedded WESL PBR extension shaders, bounded checksum-pinned texture workers and a generated GGX-prefiltered HDR studio. Tools → Visual Finishes / Studio Lighting supplies live draft editing; grayscale relief uses physical depth in mm and preserves authoritative CAD geometry. Save As carries committed, pending and tool-checkpoint resources into the destination. The original engineering material contract remains responsible for density and mass. Adapter 21 includes the added placed-part visual assignment in derived data; native schema remains 1 with omitted additive defaults. One newly locked crate is `image-webp` 0.2.4; the direct image/half/sha2 dependencies retain their existing locked versions and all license notices are bundled. See [visual finishes](visual-finishes.md).

Final adapter 21 / solver 12 source checks on 2026-10-11 passed: workspace formatting, strict locked workspace Clippy, and `scripts/package-macos.sh`. The embedded finish shader compiled with pinned Bevy 0.20 WESL imports and Naga 30 IR validation for environment/clearcoat configurations with and without SSAO/depth/normal prepasses. `dist/TCAD.app` contains distinct `TCAD`, `tcad-worker` and `tcad-cli` executables. Code is committed as `f73f5f0`; verification does not establish visual acceptance. No tests, probes or UI smoke runs were performed. The existing `proc-macro-error2` 2.0.1 future-incompatibility notice remains.

## Settings integration — 2026-10-11

A searchable, compact, resizable **Settings…** dialog now groups current-model and personal application preferences. Edit/Tools and Cmd/Ctrl+, open it. Model contract v1 saves display units/precision, evaluation tolerance and new-part finish identity in `Document.settings`; model lighting uses the existing appearance contract. Personal contract v1 stores UI scale, navigation, presentation watching, recovery enablement/interval and portable new-model defaults independently in `settings.json`. Valid legacy navigation preferences migrate once. Model defaults stay omitted to preserve legacy source fingerprints, and the CLI uses saved model tolerance unless explicitly overridden.

Settings edits share the current validated source/undo behavior and checkpoint Cancel. Application-only editing during an existing tool leaves its model draft untouched. Unrelated source actions are blocked while Settings is open; asynchronous STEP import replies remain queued. Dimension annotations and inspection follow model display units without rescaling canonical source. Recovery reloads an unsaved committed snapshot when there is no draft, against the matching saved base. Semantic fields, category/actions and CSS sizing/positioning keep the default modal replaceable. See [settings](settings.md).

Final `cargo fmt --all --check`, `cargo clippy --workspace --locked -- -D warnings` and `scripts/package-macos.sh` passed for this integration. The refreshed package is `dist/TCAD.app` with distinct `TCAD`, `tcad-worker` and `tcad-cli` executables. No tests, probes, CLI CAD exercises or manual/UI smoke checks were written or run. Interactive acceptance remains with the user. The existing dependency future-incompatibility notice remains `proc-macro-error2` 2.0.1.

## Localized operation previews — 2026-10-11

Adapter 22 adds application-owned operation-preview contract 1 to the worker
evaluation protocol. A focused feature compares its evaluated input and output;
general source drafts compare their starting checkpoint. Disposable added and
removed meshes retain evaluation, feature and baseline identity, including
configured definition mesh keys. Boolean cuts use their left operand as stock.
The viewport draws the neutral result once, with localized translucent material
overlays instead of a complete committed/draft pair. Existing feature editors
request their preview without changing source; pending or failed drafts retain
only a matching last-valid display cache. Original and History suppress these
overlays. Selection uses the authored input topology independently.

Native exact differences are preferred. Bounded tessellated differences are
labeled display approximations, never modeling or export shapes. Unavailable
comparisons and worker transport limits preserve valid CAD evaluation and leave
affected geometry neutral. The cumulative serialized preview cache is limited
to 16 MiB; per-comparison geometry and BSP work also have explicit bounds.
Overlapping edited surface trims and occurrence-only assembly movement remain
outside the localized comparison scope. See [presentation](presentation.md)
for semantic roles, independent added/removed colors, opacity and outlines.

Final source integration passed `cargo fmt --all -- --check`,
`cargo clippy --workspace --locked -- -D warnings`, `git diff --check` and
`scripts/package-macos.sh`. The updated `dist/TCAD.app` retains distinct desktop,
worker and CLI executable names. No tests, probes or manual/UI smoke checks were
written or run; interactive acceptance remains with the user. The existing
`proc-macro-error2` 2.0.1 future-incompatibility notice remains.

## Appearance panel and user texture imports — 2026-10-11

The bundled standard and Nocturne compositions now dock a separate resizable
Appearance panel beside the existing viewport. Explicit part/base and component
instance selectors drive assignment. Customize clones the assigned finish and
assigns it in one shared draft; shared-library editing is an explicit action.
Colors, grayscale bump/height and other texture maps, physical bump depth and
tile mapping have semantic controls and styled sections. The panel's stable
`workspace.appearance-panel` view publishes open/closed state; user layouts can
move or replace it. Legacy layouts retain the property-manager adapter. Source
finishes remain independent of engineering material and retain contract 1.

Texture import pins the exact bounded image buffer validated by the desktop,
without an unbounded second read of the external file. PNG/JPEG/TIFF/WebP and
existing asynchronous resource validation remain supported. Recovery snapshots
now record the original asset root, and manual recovery validates and copies
committed/draft pinned resources before replacing the active model. Direct
feature editing is blocked during an appearance transaction, preserving its
checkpoint and shared Apply/Cancel semantics.

Final `cargo fmt --all -- --check`,
`cargo clippy --workspace --locked -- -D warnings`, `git diff --check` and
`scripts/package-macos.sh` passed. The refreshed `dist/TCAD.app` retains distinct
`TCAD`, `tcad-worker` and `tcad-cli` executable names. No tests, probes or manual
UI smoke checks were performed; interactive acceptance remains with the user.
The existing `proc-macro-error2` 2.0.1 future-incompatibility notice remains.

## Astra kernel reliability work — 2026-10-11

The user resumed this scope with Astra. Open CASCADE remains prohibited. Work
continues in Monstertruck and TCAD's replaceable native adapter; compilation,
formatting and linting are allowed, but tests, runtime probes and manual/UI
smoke checks remain prohibited. Interactive acceptance stays with the user.

The scope is solid modeling reliability and its necessary shared editor/worker
integration. Continue using Monstertruck and application-owned contracts. Keep
TCAD private. A public upstream dependency fork is authorized, but must contain
only dependency work, without private TCAD source or design data. This handoff
does not relax the no-tests/no-probes/no-manual-smoke-check policy.

### Concrete implementation scope

1. **Join/cut/common and feature contact.** Correct material classification for
   outward/inverted faces, cavities and disconnected bodies; distinguish valid
   empty results from algorithm failures. Support face-attached bosses,
   overhangs, differently partitioned coincident outlines, blind/through/side
   pockets and repeated operations over every scoped solid. Preserve exact
   native intersection curves and shared topology. Repair coplanar/tangent
   boundary handling without perturbing source geometry to manufacture overlap.
2. **Constant-radius fillets.** Support practical convex/concave straight and
   circular/curved edges, complete connected selections, hole mouths, adjoining
   corners and operations after pockets, rounds or bevels. Construct real native
   tangent boundaries and blends; check corner joins, radius collisions and
   self-intersections before publication. Existing simultaneous planar stock
   rounds remain available. Finish common geometry coverage rather than treating
   a new command label or one bounded rim family as completion.
3. **Chamfers.** Preserve equal distance, two distances and distance/angle on
   connected straight and curved chains, hole mouths and previously edited
   stock. Repair shared corner closures, split-edge chains, interacting pocket
   boundaries and deterministic Distance 1 face semantics. Preserve source
   containment relationships and account for every selected rim during collision
   classification.
4. **Shell, surface offset and thicken.** Extend to everyday turned/curved and
   blended bodies, pockets and multiple openings. Offset along true oriented
   normals, intersect adjacent offset carriers, reconstruct rims and cavities,
   and handle thickness collapse explicitly. Exact cylinder/cone/sphere cases
   are useful foundations, but mixed carriers and offset joins remain required.
5. **Integration invariants.** Current validated results gate Apply; one feature
   remains one undoable operation; Cancel restores the checkpoint. Retain f64
   authority, explicit units/tolerances, construction-based topology references,
   predictable failure diagnostics, worker resource limits and stale-result
   rejection. Update kernel fingerprints and cache compatibility when behavior
   changes. Keep selection and localized operation previews consistent with the
   actual input/output geometry.

Compile, format, strictly lint, package, document supported modes and commit/push
cohesive completed changes. Interactive acceptance stays with the user. Do not
claim “rock solid” or completion from compilation alone. Preserve unsupported
diagnostics for genuine geometric singularities instead of unchanged-success,
mesh CAD substitutes, sampled intersection guides as authority, or unexplained
tolerance inflation. UI styling, appearance expansion, drawings, assemblies and
new unrelated feature families are outside this focused implementation.

### Current implementation state

The public dependency fork is `https://github.com/ens1/monstertruck`, branch
`tcad-modeling-reliability`, pinned by TCAD to
`e52aab0677610f65c647e7f8dc0829efe2cb1318`. Its parent code is the published
Monstertruck 0.4.1 revision. The fork contains only generic dependency work,
retains Apache-2.0 licensing, and removes an unrelated private `.blueprints`
submodule that otherwise prevented public Cargo dependency resolution. The
separate dependency checkout remains at
`/Users/tyler/Software/Personal/tcad-dependencies/monstertruck`.

Dependency checkpoint `bbbdf81b` corrects complete material-domain Boolean
classification, oriented cavities/disconnected shells, empty/self results,
parameter-domain loop area handling and untouched closed-component indexing.
It adds bounded triangle-interference/chain construction, fixed-tolerance
native projection checks, explicit ambiguity failures and stage-specific
errors. Search tessellations and intersection guides remain derived aids;
output boundaries retain native supporting surfaces. Dependency checkpoint
`e52aab06` adds an analytic planar arrangement for complete straight-edged
faces, including coplanar overlap, nonconvex stock, holes, cavities and
complemented operands. It merges adjacent selected cells, conforms shared
native topology and preserves absolute source carriers. Complete loop
admission, fixed-precision arithmetic, complete-link vertex matching and
bounded predicate work reject uncertain cases. General curved tangent/coplanar
SSI and globally certified freeform intersection tracing remain incomplete.

TCAD now has set-correct multibody Boolean dispatch and valid empty body-set
results. Exact planar contact joins retain native line/circle boundaries,
including partial overlap, overhang, holes and different edge partitions.
Shared split vertices/edges are reconstructed through adjoining faces and
carry source/interval/component provenance. Extrude, Revolve, Loft, Sweep, Rib
and Combine Union share this construction. Unsupported carriers and ambiguous
or nonmanifold contact arrangements still fail explicitly.

The new edge-treatment paths are registered and integrated before the existing
simultaneous convex constructors. They add local straight convex/concave
wedges, convex-void pocket/cavity chains, and complete cylindrical/conical
rims with exact toroidal fillets or conical bevels. Every candidate keeps a
private history snapshot. Later connected rounds may preserve separated
previous blends and curved trims on supporting planar walls; only the newly
selected endpoint neighborhoods require straight three-plane topology. Exact
affine sphere/torus/revolution enclosures protect retained geometry, including
nonuniform transforms and composite native curves. Outward interval arithmetic
covers affine support, inverse transforms, parameter evaluation and rational
subdivision in collision proofs; guide curves and rounded analytic-to-rational
conversions cannot certify separation. Native line-revolution walls retain
their original carrier, use actual rim frames for construction, and enclose
the changed analytic strip independently with outward Rodrigues evaluation.
Full positive tensor-control displacement bounds account for rounded profile
weights. Revolved cap sectors coalesce transactionally into circular disks or
annuli, retaining rim topology and explicit Distance 1 source relationships.
See `docs/edge-treatments.md` for precise limits.

Surface Offset/Thicken recognizes exact plane/cylinder/cone/sphere/torus
carriers. Curved Shell includes conical/cylindrical walls with intersected cap
offsets and spherical shells. A mixed turned-shell constructor solves bounded
line/circle meridian offset joins and preserves exact native revolution
geometry. Opening-cap radial intervals must nest correctly and remain clear of
nonincident meridians. Fully rounded convex envelopes now support mixed
plane/cylinder/sphere shelling through a complete convex-core normal-map
certificate; separated rounded enclosed cavities use signed-distance
containment and support-axis separation for the complete offset collar.
Surface Offset/Thicken also cover these closed envelopes and proven planar
rational carriers. Offset layers, thickened rims and normal connectors have
explicit source-derived references. Open blended-pocket networks, mixed-radius
rounded envelopes, topology-changing offsets and general freeform offsets
remain unsupported.

The backend fingerprint is now
`monstertruck/0.4.1+e52aab067761;adapter/24;solver/tcad-native-lm/12` in both
adapter and desktop. Prior derived caches are invalidated, while saved source
schema and authored definitions stay unchanged. Ambiguous old topology
references require explicit repair instead of silent retargeting. Split faces
and edges use complete canonical source-boundary and source-neighborhood
signatures. Original vertex anchors must be unique among the inputs; no native
edge direction, position or traversal index supplies a persistent identity.
Unknown-source carrier barriers survive intermediate Boolean steps. Ambiguous
components refuse instead of borrowing a positional fallback identity.

### Verification and publication state

The fork changes passed library compilation, strict Clippy with default and
no-default features, targeted nightly rustfmt checks, and `git diff --check`.
The final pinned dependency and TCAD workspace libraries/binaries compiled
together. `cargo clippy --workspace --lib --bins --locked -- -D warnings`,
`cargo fmt --all -- --check`, `git diff --check` and
`scripts/package-macos.sh` passed for both implementation checkpoints. The refreshed
`dist/TCAD.app` contains the distinct `TCAD`, `tcad-worker` and `tcad-cli`
executables. The existing `proc-macro-error2` 2.0.1 future-incompatibility notice
remains. No tests, assertion probes or manual/UI smoke tests were written or run.

The first private checkpoint is `e67d105`, pushed to the private origin.
The second checkpoint adds the rounded-body, repeated-round, native revolved-rim
and planar Boolean work described above. Source review also strengthened
carrier-ancestry propagation between Boolean steps, unknown-source barriers,
duplicate-identity validation, bounded ancestry serialization and canonical
component naming. Final workspace checks and packaging are complete. The
public dependency changes are pushed; this document accompanies the second
private implementation commit. This is not a claim of universal kernel coverage;
interactive geometric acceptance remains with the user. The unsupported
intersection, mixed-blend and offset families listed above remain kernel work,
not work that presentation or feature-editor changes can resolve.

## macOS launch packaging repair — 2026-10-11

The user reported that the refreshed bundle failed to open. Existing macOS
crash reports identify `SIGKILL (Code Signature Invalid)` with `CODESIGNING /
Invalid Page` in dyld, before desktop startup. Static bundle verification also
failed because the linker signature did not seal the assembled app resources.
The previous packaging script overwrote executables in an existing bundle and
never signed the completed app.

`scripts/package-macos.sh` now builds in a fresh staging directory, preserves
distinct executable names, ad-hoc signs the worker and CLI before sealing the
whole bundle, and verifies each executable plus the complete resource seal.
The existing package is replaced only after successful verification; failed
publication restores the preceding package. Fresh files also avoid reusing
executable inodes associated with stale macOS code-signature pages.
`sh -n scripts/package-macos.sh`, `git diff --check` and the complete
compile/sign/verify packaging workflow passed. After publication,
`codesign --verify --deep --strict --verbose=2 dist/TCAD.app` and independent
worker/CLI signature checks also passed. No tests, app launch or interactive
acceptance were performed; the user retains interactive acceptance.


## Sol application pass before further kernel work — 2026-10-11

The user requested the Sol application work before the remaining Astra geometry
work. This pass integrates native incidence-based complete-rim/loop, tangent-chain
and face-boundary selection; per-edge manager focus/removal; sketch relation
filtering and exact current authored-equation feedback; explicit paired reference
repair; and compact independently styleable workspace controls. See
[modeling workflows](modeling-workflows.md) for implementation and remaining bounds.

The part-feature evaluator now returns targeted current-source construction
failures with valid=false, actual available upstream/history geometry and blocked
dependent features. This supports repair of a failed draft without discarding its
original modeling checkpoint. Explicit pre-feature stage picking finds consumed
upstream outputs. Atomic source replacements check the exact expected record,
paired body/sketch-element bindings, type, order and document dependencies. Apply
requires current validation; Cancel restores the starting checkpoint. Structural,
resource, parameter and assembly failures outside feature construction retain
explicit worker errors without fabricated targets or stale geometry.

The sketch relation manager retains exact successful SolveReport data only for
its matching source and exposes current authored residual vectors separately.
Failed solves have unknown DOF. No minimal-conflict claim is made. Geometry,
handles and annotations emphasize selected or currently unsatisfied participants
without mutating the actual selection. Field/command adapters share sketch
history and source validation. The default rows are compact, with full labels
available through semantics and hover text.

The default toolbar has theme-controlled ready/selected/editing/inspection heights,
while selected history/tree rows no longer grow. Viewport toolbar, start controls,
cube, candidate status, isolation and navigation help now have registered semantic
renderers and independent layout bindings. Repair/sketch managers and all their
choice/field/action controls publish the same presentation contracts as native
widgets. Custom Actions bindings cannot switch away from an active editor.

Backend fingerprint:
`monstertruck/0.4.1+e52aab067761;adapter/25;solver/tcad-native-lm/12`.
The public Monstertruck fork revision and native solver algorithms are unchanged.
Remaining curved intersections, general connected blends and freeform offset/
loft proofs are still kernel work for the later Astra pass. Broader mechanical
CAD coverage remains incomplete; completing this application pass does not close
those families. No tests, assertion probes or interactive smoke tests were
introduced or performed.

### Validation and publication

Workspace library/binary `cargo check --workspace --lib --bins --locked` passed.
Final `cargo clippy --workspace --lib --bins --locked -- -D warnings`,
`cargo fmt --all -- --check`, `git diff --check` and packaging-script syntax
checks passed. The existing `proc-macro-error2` future-incompatibility notice
remains. `scripts/package-macos.sh` compiled the entire workspace, created fresh
executables, signed and strictly verified the nested worker/CLI and complete
bundle, then published `dist/TCAD.app`. It contains `TCAD`, `tcad-worker` and
`tcad-cli`; no app launch or interactive acceptance was performed.

Implementation checkpoint `a4270ee` is committed and pushed to the canonical
private origin. Repository visibility was verified as private. The public
Monstertruck fork received no changes during this pass. The refreshed bundle is
ready for the user's interactive acceptance, with the geometric limitations
documented above still in force.

## Astra native geometry extension — 2026-10-11

This pass continues the focused kernel scope after the Sol workflow pass.
Monstertruck remains the kernel, with no Open CASCADE. The public fork remains
pinned at `e52aab0677610f65c647e7f8dc0829efe2cb1318`; this pass's specialized
constructors and history integration live in the private TCAD native adapter.
No additional repository or public source publication was needed.

Adapter 26 adds common-axis layered line/arc extrusion Booleans with one exact
planar arrangement shared across heights. Different extents, holes, nonconvex
profiles and repeated layered results use native curves and surfaces, shared
edges, explicit source-interval/plane/endpoint ancestry, fixed native tolerance
and bounded predicate work. History-aware feature combinations, drilled-hole Boolean fallbacks and explicit
Boolean operations use this path; legacy geometry-only calls retain the existing
backend. `robust` 1.2.0 is now a direct dependency for adaptive exact orientation;
it was already locked transitively and its licenses were already bundled.

The fillet/chamfer extension treats circular chains terminating at actual radial
planes, including circular/annular sectors and half-round stock. Selected
collinear planar chains can coalesce source seams before the existing complete
corner constructor runs. Terminal and other cap edits are composed before any
face replacement. The first cohesive checkpoint is commit `43e9431`, pushed to
the private origin; its final terminal-composition correction is in the combined
integration checkpoint.

Surface Offset and Thicken now handle native tangent plane/cylinder extrusion
sheets with multiple bend radii. Their complete cross-section collars receive
interval-bounded intersection and containment checks; mid-plane thickness checks
the combined two-sided interval. Coaxial cylindrical/conical shells allow whole
lateral-contour openings and combined side/end openings, with source endpoint
ancestry on their connectors. Partial angular removal, arbitrary trims and
sharp/freeform offset joins remain unsupported.

Ruled loft charts now admit nonparallel section pairs through fixed dual-frame
certificates and actual shared-wire subdivision. Complete per-pair strip
separation uses outward rational control hulls and native shared connectors.
This also admits additional uninfluenced concave solid lofts. Multi-pair Surface
Loft requires exact ordered native coordinate-plane section slabs until complete cross-pair
intersection handling exists. Uncertain separation is distinguished from a
proof of an actual fold.

Independent source reviews caught and corrected near-parallel support equality,
uncertain intersection roots, shared-connector proof assumptions, cap ancestry,
layered split-face identities and lateral-opening connector identities. Native
allocation IDs and mesh indices do not enter the new saved construction roles.
Remaining broad limitations include general curved tangent/coplanar SSI,
freeform intersection completeness, arbitrary connected blend junctions,
variable-radius blends, topology-changing offsets and unrestricted multi-pair
loft skins. Those remain engineering work; this pass does not claim a complete
or interactively accepted production kernel.

Detailed contracts and limits:
- [Native prismatic Booleans](native-prismatic-booleans.md)
- [Edge treatments](edge-treatments.md)
- [Native offsets](native-offsets.md)
- [Loft geometry](loft-geometry.md)

### Final verification and package

After the review corrections, `cargo fmt --all -- --check`,
`cargo check --workspace --lib --bins --locked`,
`cargo clippy --workspace --lib --bins --locked -- -D warnings` and
`git diff --check` passed. `scripts/package-macos.sh` completed the locked
workspace build, plist validation, ad-hoc signing and strict nested/full bundle
signature verification. The refreshed application is `dist/TCAD.app`, with
separate `TCAD`, `tcad-worker` and `tcad-cli` executables. Both desktop and kernel
use `monstertruck/0.4.1+e52aab067761;adapter/26;solver/tcad-native-lm/12`.
The existing dependency future-incompatibility notice is `proc-macro-error2`
2.0.1. No tests, assertion probes, CLI modeling exercises, app launches or manual
UI smoke checks were performed. Interactive modeling acceptance remains with
the user. The repository was verified private before publication.

## 2026-10-11: Straight fillet ending on a cylindrical wall

Read-only inspection of the reported recovery draft identified a 3 mm fillet
on a straight top edge of an added extrusion. One endpoint joins a circular
extrusion wall at an inward sketch corner. The previous local wedge constructor
required planar terminal faces and declined this case. Correct construction
needs both a cylinder/cylinder intersection and continuation of the neighboring
arc and terminal wall; a planar end profile would change the intended geometry.

`curved_blend_proof.rs` certifies the complete native intersection branch using
outward quadratic root bounds and actual homogeneous meridian controls. The
source wall must be a complete four-edge cylindrical patch, the selected edge
must be a convex right-angle planar wedge, and native root separation, angular
extent, generator length and terminal order must remain valid. The integration
in `local_blend.rs` / `curved_blend_end.rs` retains one cylindrical fillet
surface, shares the real intersection with the continued terminal cylinder,
and preserves source ancestry without putting radius-derived coordinates in
topology identities. Complete changed-material enclosures protect other trims
and exclude the opposite cylinder branch. General oblique, conical, freeform
and connected mixed-carrier junctions remain work.

The public Monstertruck fork commit
`fc2306852a463573d35f9204ac76b1a6f712cc84` corrects intersection evaluation,
including endpoints, subdivision and parameter-boundary conversion to use both
native carriers instead of a search guide. Persistent branch halfspaces survive
serialization, transforms, inversion, cutting and content hashing. Unresolved
evaluation fails explicitly, and exact rational conversion refuses implicit
intersection guides. No private application source or recovery data was added
to the public fork. TCAD pins this revision and refreshes its dependency notices.
Both desktop and kernel now use
`monstertruck/0.4.1+fc2306852a46;adapter/27;solver/tcad-native-lm/12`.
Dependency integration checkpoint `6c60ae3` was pushed to the private origin.

Native TCAD source saving remains unchanged. STEP export of branch-constrained
intersection trims is explicitly refused before formatting or touching the
destination, because the current STEP writer would otherwise serialize the
guide as the spatial boundary and lose the selected branch. Export fidelity
for these trims remains an explicit gap.

After the final source-review corrections, workspace formatting, locked
library/binary compilation, strict Clippy (`-D warnings`) and diff checks passed.
`scripts/package-macos.sh` rebuilt `dist/TCAD.app`, validated its plist, and
completed nested and whole-bundle signature verification. The public fork's
geometry/modeling/io libraries passed stable compilation and strict lint;
nightly formatting passed. Its initial nightly compile encountered the existing
`branches 0.4.6` removed-intrinsic issue, so compilation used stable. TCAD retains
the pre-existing `proc-macro-error2 2.0.1` future-compatibility notice. No tests,
assertion probes, CLI modeling exercises, app launches or manual acceptance
checks were run. This is implementation and source-review evidence, not a
claim of interactively verified fillet behavior.

## Multiple sketch contours — 2026-10-11

`tcad.sketch_topology` version 1 preserves authored solved endpoint aliases
through section flattening and propagates shared endpoint ancestry through
matching sketch pattern transforms/instance placements. Isolated mutually
unique material endpoints can share a derived vertex within `1e-6 mm`; complete
alias-cluster distances are checked, with no transitive gap bridging. Saved
source controls, constraints and pattern definitions stay unchanged. Native
profile building and section consumers share this connectivity. Closed Bézier
members split exactly into two native edges instead of constructing a same-ID
endpoint edge. Legacy polygon controls use the same canonical coordinates.

`tcad.contours` version 2 keeps the historical `outer_members` JSON field while
allowing multiple region picks in one record. Each current boundary member must
resolve uniquely; holes select their parent material region with the holes
retained. Repeated region picks are deduplicated in first-pick order, including
thin-chain selections. All Closed Contours keeps valid closed regions despite
disconnected open chains. Truly open-only sources still fail solid construction
and require closure or Thin Feature. Branching networks and uncertain native
arrangements remain explicit gaps; no general region-arrangement solver is
claimed. Existing multi-tool boss/cut handling and scope/through-all bounds are
retained.

The worker publishes disposable outer/hole member membership through
`KernelAdapter::sketch_material_regions` and `EvaluatedSketch.material_regions`.
Only application-owned source IDs cross this boundary. The compact scrollable
picker uses current solved classification, including available source profiles
in failed feature evaluations. A parent and its holes form one choice; outer
anchors are preferred so deleting a hole does not lose an otherwise surviving
region. All, Selected, Select All, Clear and explicit loft ordering share native
controls and semantic presentation actions. `contour-list` exposes `max-height`
and `contour-option`/`contour-mode`/`contour-action` roles expose styled controls.
Semantic commands reject changed feature source and stale row indices. Pending
classification cannot become a validated region selection. Command/Shift sketch
member picks survive asynchronous sketch confirmation using a document/part/
profile-scoped deferred payload for Extrude/Cut/Revolve/Loft/Sweep presets.

Both desktop and kernel use
`monstertruck/0.4.1+fc2306852a46;adapter/28;solver/tcad-native-lm/12`.
The Monstertruck dependency revision is unchanged in this continuation.
Locked workspace library/binary compilation, workspace formatting and strict
Clippy (`-D warnings`) and diff checks passed. `scripts/package-macos.sh`
rebuilt `dist/TCAD.app`, validated its plist and verified nested and whole-bundle
signatures. The pre-existing `proc-macro-error2 2.0.1` future-compatibility
notice remains. No tests, modeling probes, app launches or manual UI acceptance
checks were run.

## Classic theme switching correction — 2026-10-11

The portable style property catalog now includes `text-overflow` with explicit
`clip` and `ellipsis` values. Classic Orange's existing CSS/JSON rows already
used ellipsis and the native control renderer already implemented it, but
candidate validation rejected the missing catalog entry. This same mismatch
blocked Classic selection, F9 presentation recovery and imported packages
inheriting the default style. Presentation schema remains 1; no theme or model
definitions are rewritten. Existing validate-before-install and Settings
preview/Apply/Cancel behavior is retained. Source review found no other
unsupported property names in either bundled theme.

Workspace formatting, locked library/binary compilation and strict Clippy
(`-D warnings`) and diff checks passed. `scripts/package-macos.sh` rebuilt
`dist/TCAD.app`, validated its plist and verified nested/app signatures. The
existing `proc-macro-error2 2.0.1` future-compatibility notice remains. No tests,
modeling probes, app launches or manual UI checks were run.

## Sketching, semantic editors and assembly manipulation — 2026-10-11

Sketch diagnostics v1 maps the native current f64 Jacobian/nullspace into each
actual source/generated point's local positional directions and publishes ordered
per-relation dependencies separately from nonpositional size equations. A dedicated
persistent worker debounces analysis and rejects outdated editor/source/document
results. Freedom markers, Dependent filtering, exact duplicate cleanup preserving
group-owned IDs and selected dimensions → reference share pending sketch edits.
Uncertain derivatives, rank, singular/current invalid geometry and explicit work
limits leave normal solver editing active with unavailable feedback; this does not
add global minimal conflicts or finite-motion certification. See
[sketch diagnostics](sketch-diagnostics.md).

All current Surface, Direct Edit, Sheet Metal, Weldment and Mold variants now
publish independently bindable typed fields, labeled source/reference choices,
ordered collections, selection/reconciliation actions, group frames and help.
Native controls render the same descriptors as custom layouts. Session/source
guards reject stale controls; generation-scoped text retains invalid input and
blocks Apply until corrected or removed. Classic rows/actions/groups and field
diagnostics have compact CSS/JSON defaults. Drawing sheet/format/non-symbol
controls and other families still have composite-editor migration work. See
[presentation](presentation.md).

Assembly manipulation v1 adds docked Move/Rotate/combined goals with explicit
unit-aware expressions, draggable numeric dials and origin/picked pivots. A
dedicated worker optimizes the transient goal and strictly validates the existing
mates/travel bounds. Translation expressions now participate as hard equations.
Accepted placements and free contact/path seeds become one shared source draft;
Apply also checks that its normal current evaluation preserves the accepted pose.
Settings defers write adoption, newer goal/source identities reject stale replies,
and Cancel restores the shared checkpoint. Root rigid participants move, including
a complete nested clicked path. Fixed/grounded, patterned, expression-driven and
fully constrained occurrences are unavailable explicitly. Free viewport dragging,
flexible subassemblies, configuration-specific pose overrides, collision avoidance
and continuous multi-turn mechanisms remain work. See
[assembly mates](assembly-mates.md).

Feature order v2 adds stable relative destination commands, dependency-safe tree
drag/drop, styled insertion markers, edge scrolling and docked semantic destination
actions. One worker draft previews the source order; Apply/Cancel/undo preserve
identities and share the original checkpoint. Editable rollback/freeze and
automatic retargeting remain work. See [feature order](feature-order.md).

Backend fingerprint is
`monstertruck/0.4.1+fc2306852a46;adapter/29;solver/tcad-native-lm/13`.
The Monstertruck dependency revision and native source schema are unchanged.

Final source review canonicalized persisted free Path/Slot/Cam seeds into the
existing 0..1 source domain: closed curves use modulo 1 and open curves use the
same clamped contact domain as mate equations. Nonfinite parameters or missing
native carriers remain errors. This preserves evaluated contact at curve seams
and travel ends while keeping accepted definitions valid.

After that correction, workspace formatting checks, locked library/binary
compilation and strict workspace Clippy (`-D warnings`) passed, as did diff
checks. `scripts/package-macos.sh` rebuilt `dist/TCAD.app`, validated its plist
and verified its nested and whole-bundle signatures. Executables remain distinct
as `TCAD`, `tcad-worker` and `tcad-cli`. The existing `proc-macro-error2 2.0.1`
future-compatibility notice remains. No tests, probes, app/CLI modeling launches
or manual/UI acceptance checks were run. Interactive acceptance remains with
the user. The implementation checkpoint and follow-up correction are committed
and pushed to the private canonical origin.

## Model gestures, configuration poses and editor migration — 2026-10-11

Assembly gesture contract 1 adds true viewport component translation on the
view plane or document axes and rotation on view/axis rings. An owned pointer
capture retains full occurrence/viewport identity and f64 starting geometry.
The existing mate-aware persistent worker solves each current sampled goal;
release submits its final goal, Escape restores the gesture-start draft, and
Cancel restores the original operation checkpoint. Camera gestures, selection
menus, Settings, layout replacement and focus/size changes relinquish or reject
conflicting ownership. A disposable placement display uses unchanged native
meshes when no in-context dependencies/component patterns require native rebuild;
it is never a valid document evaluation or an Apply result. Current complete
evaluation must still preserve accepted poses before confirmation. Flexible
subassemblies, collision avoidance, continuous winding and large-assembly sparse
performance remain work.

Configuration contract 4 adds inherited per-axis component placements and free
Path/Slot/Cam seed maps. Base/Named scoped writes, detail controls, CSV/JSON tables,
namespace remapping and instance/mate/pattern deletion cover the new stable IDs.
Expression-driven translations and generated patterns retain their drivers.
Omitted-empty maps preserve old source shapes and versions 1–3 remain readable.
Fields use current editor/source identity, exact unit entry and generation-scoped
invalid-text diagnostics. See [configurations](configurations.md).

Drawing sheets, views, formats/title cells, dimensions, annotations and tables
now use independently bound inspector fields/actions. Anonymous table/note rows
retain session slot identities; authored records retain saved IDs. Hidden invalid
input remains associated with the source item and blocks confirmation until
corrected or explicitly removed. Existing symbol descriptors and manufacturing
vector geometry remain unchanged. Common Fillet, Chamfer, Shell, Draft, Pattern,
Transform, Scale, Boolean and Mirror controls join the five previously migrated
modeling families. Dynamic actions reject changed source/selection snapshots,
body changes reconcile dependent faces, and bounded skip enumeration uses set
membership. Other modeling editor families remain to migrate.

Shared native value controls honor CSS row/column layout, width/height/min/max
and minimum targets, semantic states and label ellipsis. Choice menus add
search and virtual scrolling. Projection, picking and cursor-anchored zoom match
the shared texture's aspect when user layouts stretch or duplicate viewports;
those viewports still share a camera/render texture. Backend fingerprint is
`monstertruck/0.4.1+fc2306852a46;adapter/30;solver/tcad-native-lm/14`.
No Monstertruck dependency revision or repository privacy change was needed.

Final source review also preserves source compatibility for the renderer's
ambiguous literal `base` configuration ID: the new pose inspector reports the
collision and declines those ambiguous controls rather than silently writing
Base. Core identities, evaluation, saved definitions and other named scopes
remain unchanged.

After the gesture ownership, rotation sampling and scope safety corrections,
`cargo fmt --all`, locked workspace library/binary compilation, strict workspace
Clippy (`-D warnings`) and `git diff --check` passed. `scripts/package-macos.sh`
rebuilt `dist/TCAD.app`, validated its plist and verified nested and whole-bundle
signatures. Executable names remain `TCAD`, `tcad-worker` and `tcad-cli`. The
existing `proc-macro-error2 2.0.1` future-compatibility notice remains. No tests,
probes, app/CLI modeling launches or manual/UI acceptance checks were run.
The implementation and verification record are committed and pushed to the
private canonical origin; interactive acceptance remains with the user.

## Bump-driven machining grain and studio fidelity — 2026-10-11

The user's flat-metal screenshot and machined-aluminum reference prompted a
rendering update. Their clarification that bump maps must drive grain supersedes
an initial procedural shader-grain plan. The shipped shader has no procedural
grain overlay: height pixels drive the relief.

Appearance contract **2** adds `Finish.builtin_bump` (`brushed` / `none`) and
finite 0–1 `Finish.anisotropy`. Aluminum defaults to the original bundled
`assets/appearance/brushed-height-v1.png`, a seamless 1024-square, 16-bit grayscale
map authored by `scripts/generate-finish-maps.py`. Painted RAL 1015 defaults to no
bump and isotropic response. Imported height resources replace the built-in map,
even while loading or failed; Clear restores the selected built-in. These maps
share the asynchronous red-channel/float-mip/R16Float decode path. Existing tile
size, direction and depth control actual map sampling. Texture decoding readiness
still gates Apply; Cancel and common undo behavior are preserved.

The finish shader differentiates filtered grayscale height at its projected
pixel footprint, retaining physical millimetre gradients across image resolution
and zoom. Anisotropic reflection directions follow rotated part-local texture
axes without mesh UVs or tangents. Clearcoat uses carrier/imported normals for
broad reflections. The studio now supplies brighter walls and five broad cards,
with a stronger environment and weaker direct lights; cubemap handedness and
rotation align reflected cards with direct lights. Full 32-pixel diffuse and
128-pixel/eight-mip specular faces filter asynchronously with 256 samples while a
coarse startup studio is available. CAD surfaces and sharp edges are unchanged;
real fillets/chamfers remain responsible for geometric edge highlights.

Older explicit finishes retain stored colors, PBR values, imports and mapping;
omitted v2 fields gain brushed/0.45 defaults. Both native docked and custom semantic
controls expose built-in selection, anisotropy, choices, bounds and help through
the common validated command path. No dependency/kernel revision changed.

Completed source checks: `cargo check --workspace --locked`,
`cargo fmt --all -- --check`, `cargo clippy --workspace --locked -- -D warnings`,
`python3 -m py_compile scripts/generate-finish-maps.py`, and `git diff --check`.
The actual finish shader compiled against pinned Bevy 0.20 WESL imports and
passed Naga 30 IR validation for plain, clearcoat/environment/SSAO, anisotropic,
and depth/normal-prepass variants (Rust 1.99 compiler utility). The HDR camera uses
a separate tone pass; in-shader tone mapping is not an active TCAD variant.
`scripts/package-macos.sh` rebuilt and signature-verified `dist/TCAD.app` with
`TCAD`, `tcad-worker` and `tcad-cli`. The repository remains private. The existing
`proc-macro-error2` future-incompatibility notice remains. No tests, assertion
probes, model exercises, app launches or UI smoke runs were performed; visual
acceptance is still the user's next step.

## Default part edges and hover center guides — 2026-10-11

The user requested visible part edges by default and clarified that nearby hover
should expose both circular centers/axes and straight-edge midpoint guides.
Personal application preferences version 1 now add `show_part_edges`,
`hover_centerlines` and `hover_midpoints`, all default true and readable from older
files through additive defaults. **Settings → Application → Model display**
provides independent semantic toggles using the existing staged personal
preview/Apply/Cancel path. Portable CSS controls visibility, colors, widths,
proximity, guide extent and marks; bundled themes set suitable colors without
forcing visibility over preferences.

The desktop retains native boundary line assets over the same displayed
bodies/occurrences/history station, with depth testing and a small surface bias.
Styles address `part-edge` roles and encoded occurrence/body/construction IDs.
Assets rebuild for evaluation, theme, viewport ancestry, visibility or render
scale changes; unchanged navigation reuses them. Existing selection outlines and
localized operation feedback remain independent.

Kernel adapter **31** publishes `EdgeData.center` version 1: exact native bounded
line endpoints or a proved circular center/normal/radius/radial frame. Unsupported
elliptical, spline or implicit carriers without a supported proof omit optional
metadata; centers are never fit from display tessellation. Geometry/source
operations are unchanged, and both desktop/worker fingerprints match. No
Monstertruck revision or public fork change was needed.

The new guide renderer locates nearby visible edge samples, then draws circular
center marks, in-plane centerlines and normal axes, or straight midpoint marks
and perpendicular guides. These noninteractive overlays never open menus or
mutate modeling intent. They follow current occurrence placement, inspected
history and feature-input picking; unrelated opaque assembly occurrences still
occlude input-edge guides. Pointer exit, popup/cube/toolbar ownership, drawing
capture, navigation and component dragging clear feedback. Cached hits/misses
invalidate with source/view/selection/theme/preferences; each query shares
200,000 segment checks and 200,000 triangle checks over up to eight candidates.
Extreme display work conservatively omits optional feedback. Transient semantic
snapshots include evaluation/view context and reject stale placements.

Completed source checks: locked workspace compilation, focused desktop/kernel
compilation, `cargo fmt --all -- --check`,
`cargo clippy --workspace --locked -- -D warnings`, and `git diff --check`.
No tests, probes, model exercises, app launches or UI smoke checks were performed;
interactive acceptance remains the user's work. The existing
`proc-macro-error2` future-incompatibility notice remains.

Signed packaging completed for implementation commit `1995454`:
`scripts/package-macos.sh` rebuilt the locked workspace, validated the bundle
plist, signed all three distinct executables and signature-verified
`dist/TCAD.app`. The implementation was pushed to the private canonical origin.
No app was launched or tested interactively.

## 2026-10-11 manufacturing and visible weld checkpoint

Sheet contract v2 adds exact chained signed flanges through 180°, open hems,
jogs with developed bend schedules, and closed base-normal cuts on parallel
sketch/model-face planes. Weldment contract v2 adds circular stock and developed
lengths, joint gaps and trim-to-primary butt corners, inner/outer/recessed caps,
and native cap/gusset stock rows. Weld bead contract v1 deposits real crowned
fillet/butt B-reps along supported associative line/circle seams in parts and
assemblies. Continuous/intermittent segments retain exact analytic volume and
length metadata. Separate assembly ownership, configured suppression (config
contract v5), optical/material assignments, reference repair, library namespace
and Make Unique retargeting, native edges, STL/STEP, assembled drawings and
boundary-based mass inspection are integrated. Exploded views and saved native
assembly collision/clearance selections currently omit assembly-owned beads;
assembled part/component stock remains the inspection target. Geometry bounds
and outstanding manufacturing gaps are recorded in manufacturing-geometry.md
and mechanical-cad-coverage.md. Backend adapter revision is 32.

Completed: `cargo check --workspace --locked`, `cargo fmt --all --check`,
`cargo clippy --workspace --locked -- -D warnings`, and `git diff --check`.
No tests, probes, app launches or UI smoke checks. Packaging will follow the
requested assembly continuation. The existing dependency future-incompatibility
notice for proc-macro-error2 remains.

## 2026-10-11 assembly authoring continuation

Rigid-group contract v1 stores captured f64 relative poses for 2–128 ordinary
direct components and expands into source-owned native lock equations during
solving. Actual link residual/rank and component DOF diagnostics remain derived.
Configuration contract v6 adds inherited group suppression; source namespaces,
reserved identities, deletion and current-evaluation capture are guarded.
Motion contract v2/NativeSolver15 handles all selected component goals jointly
with temporary relative-pose equations and shared picked/origin pivot transforms;
pose-only display caches omit assembly welds until current joint geometry has
been reevaluated. Copy With Mates v1 creates ordinary editable occurrences and
mates from validated poses, with internal/group constraints and explicit outside
endpoint/native-reference rebinding. Insertions are shared source; initial named
pose hints and later suppression retain their explicit scopes. All editors use
semantic fields/actions, protected shared source, validated Apply, checkpoint
Cancel and one undo. Component preselection/highlighting/fit and assembly
breadcrumbs retain consistent source context. Operation Preview v2 adds bounded
native-derived occurrence addition/removal caches so copied/patterned components
highlight without recoloring their originals. Backend adapter revision is 33.

Completed: `cargo check --workspace --locked`, `cargo fmt --all --check`,
`cargo clippy --workspace --locked -- -D warnings`, and `git diff --check`.
No tests, probes, model exercises or UI launches. Flexible occurrence solves,
continuous mechanical winding and collision-stopped movement remain gaps.
Source review repaired competing-command/source-style/editor-close/unchanged
confirmation and stale breadcrumb-context issues before the final checks.

Signed packaging completed for manufacturing/assembly commits `3dd8a85` and
`55f844a`: `scripts/package-macos.sh` compiled the locked workspace, validated
the bundle plist, signed the distinct `TCAD`, `tcad-worker` and `tcad-cli`
executables, and signature-verified the completed `dist/TCAD.app` before replacing
the previous package. Both implementations are pushed to the private canonical
origin. No application launch or interactive acceptance was performed.
