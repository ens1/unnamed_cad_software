# Associative reference curves

Reference Curves contract **1** saves application-owned geometric intent under `Operation::ReferenceCurve`. It is a reusable construction feature rather than a solid, imported vendor object, renderer line or profile primitive. Definitions remain editable in the docked property manager and use the common feature draft, current worker validation, checkpoint Cancel and one undoable Apply. Feature and source identities persist in local files and component snapshots; parameter references participate in normal dependency/cycle diagnostics.

## Composite Curve

An ordered list of up to 256 retained sources produces one open or explicitly closed native wire. Nested source expansion is capped at **4096 native pieces**, checked before growing the flattened construction. A source is one exact native sketch member, one model/reference-curve edge with its owning body and construction reference, or a whole prior reference curve. Each list entry has an independent persistent ID and reversal flag. Order, reversal, closure and join tolerance are editable. Closed sketch circles/ellipses retain their native rational pieces rather than becoming sampled circles.

The source geometry is retained exactly. Authoring supplies the order; evaluation verifies adjacent endpoints against an explicit **1e-10–1e-6 mm** join tolerance and builds actual shared native junction vertices. It rejects disconnected sources, incorrect open/closed declarations and repeated junctions rather than choosing a branch or repairing a gap. Model edges must resolve uniquely by construction identity. Implicit intersection curves are rejected until their composite evaluation can use a fallible supporting-carrier resolver. Sketch construction members can supply reference geometry; suppressed members cannot. A whole prior curve must supply one native path.

Constructed edge roles retain the composite entry identity and normalized source ancestry. They do not use coordinates, centroids, tessellation signatures or kernel allocation IDs. Compatible source geometry edits preserve references; removed/replaced source entries can retire dependent references and require explicit repair. This feature does not automatically sort an unordered selection, heal gaps, impose smooth joins or manufacture tangent/curvature continuity between unrelated sources.

## Curve Through Points

Two to 256 ordered associative point sources produce straight segments or an interpolating spatial cubic spline. A closed curve requires at least three points. Sources reuse the shared point controls: coordinate expressions, sketch points, datum/coordinate-system origins, plane/axis constructions and bounded native model-edge parameters. Each authored point record has a stable ID. Point reordering, insertion, deletion, source editing and closure are saved intent.

Straight mode joins the exact evaluated points. Cubic mode uses chord-length parameters and natural zero-second-derivative ends for an open curve; a closed curve uses periodic second derivatives. A finite linear solve constructs exact cubic Bézier segments with shared topology at the authored points. Interpolation residuals and adjacent separation are validated at runtime. Source points must be separated by more than **1e-7 mm**. Each piece is named from its two endpoint record identities and interpolation constructor.

The curve is three-dimensional; the source does not claim a planar sketch or create hidden spatial sketch constraints. Free endpoint tangents, arbitrary degree, tension controls, least-squares fitting, and curve-from-file import are not implemented here. An interpolating reference curve may cross itself; consuming features must prove their own sweep and boundary validity. Piecewise native parameters are not arc-length fractions.

## Helix / Spiral

A retained axis, radial direction, start radius, signed pitch per revolution, positive revolution count, start angle and optional end radius produce a true analytic cylindrical/conical helix or planar spiral. The radial direction reuses explicit, associative axis, plane-normal and two-point direction sources; evaluation projects it perpendicular to the axis and rejects an unstable parallel choice. Reverse changes winding; signed pitch determines axial travel. End-radius interpolation is linear along the authored revolutions. Positive start/end radii must exceed **1e-7 mm** and the turn count is at most **256**. Start-angle phase reduction rejects magnitudes greater than one million full turns.

For nonzero pitch or changing radius, the native carrier is a straight U/V parameter-space line on an analytic revolution surface. Its angle and height vary linearly, so nonzero pitch is a true constant-pitch helix. A varying radius with zero pitch is a true planar Archimedean spiral. It is not a rational half-angle twist law, fitted NURBS, control polygon or polyline. Quarter-turn constructor stations bound native edges; their roles depend on the station and owning feature, not evaluated spatial locations. Rendered line samples are disposable display geometry.

Constant-radius zero-pitch arcs/circles use exact positive-weight quadratic rational arcs in the authored arbitrary axis/radial frame, phase and winding. That circular case can use the existing circle proofs, tangent sweep and rational sketch conversion. Its native rational parameter is not a uniform angular or arc-length fraction.

A zero-pitch, constant-radius path supports at most one revolution; a full revolution is explicitly closed. Repeated coincident circular turns are rejected. Variable-pitch laws, apex collapse to zero radius, swept/nonlinear taper, arbitrary equations and global self-intersection certification remain work.

## Consumers and presentation

The evaluator publishes application-owned `Evaluated`/`Piece` records and, for a helix, its finite analytic frame/radius/height/angle description. Analytic pieces also publish a conservative second-derivative norm over the normalized native parameter. This is derived from the actual straight parameter-space path, straight revolution generatrix and finite affine carrier, so it survives composite copying and reversal. It supplies a chord-error subdivision bound; it is disposable display metadata. Analytic reusable sources lacking this carrier proof are rejected instead of relying on samples to guess their behavior. The native wires stay inside the replaceable kernel adapter. Curve edges are shown and picked in the same model viewport with their owning feature and actual construction references; they do not enter solid mass/volume calculations.

The shared native curve sampler supplies two points for straight edges and bounded native adaptive subdivision for rational edges, seeded at their knot intervals. The native rational routine requires display tolerance at least **1e-6 mm**; tighter requests are rejected rather than silently relaxed. Intersection edge samples use fallible native projection and verify finite parameters/points and residuals on both supporting surfaces within **1e-6 mm**. Bounded midpoint and quarter-point refinement estimates their displayed chord deviation; it is not a proved global intersection approximation bound. A failed projection rejects the derived display instead of substituting the intersection search guide. The sampler bounds native evaluations, output points, and refinement depth/cycles before growth. Rendered points remain disposable and do not become saved geometric authority.

Solid sweeps can choose a reusable reference path directly. Saved source order determines its direction, and Sweep's native trim/reversal, roll and supported orientation/thin/body-scope options apply. Sketch-only Path Member/Path Start Vertex controls are not applicable to reference curves. Exact line/B-spline/NURBS composites and through-point splines still require the existing sweep's monotonic/clearance/frame proofs. Circular paths use their exact circle proofs. General nonmonotonic spline moving frames and analytic helix sweeps are not delivered by accepting the reference path.

The sweep converter rejects analytic parameter/intersection carriers **before** native rational lifting, which could otherwise fit them. Exact reusable rational edges can feed Convert Entities and bounded edge-parameter point sources through the shared selection/provenance path. True helix/spiral projection into a planar rational sketch and rational-only sweeps remain explicit unsupported cases. The current native surface enum cannot carry the combined rotation and axial translation of an exact screw-sweep wall. General spline transport likewise needs a nonrational moving-frame carrier. Implementing those consumers requires native analytic surface evaluation/derivatives, trimming, inverse/search, meshing and exchange support; homotopy conversion or rational lifting would fit them. These are engineering gaps rather than accepted product limits. Reference-curve-specific STEP/wire export, curve-driven patterns, spatial curve constraints, projected/intersection/surface/composite path repair and further analytic consumers remain work.

## Source and verification

- [Core definitions](../crates/cad-core/src/reference_curves.rs)
- [Exact native construction](../crates/cad-kernel/src/reference_curves.rs)
- [Derived native curve display](../crates/cad-kernel/src/curve_sampling.rs)
- [Docked editor](../crates/cad-desktop/src/reference_curves_ui.rs)
- [Solid sweep consumer](../crates/cad-kernel/src/swept.rs)

This describes saved implementation and its explicit bounds. Integrated compilation/linting/packaging and interactive acceptance are separate evidence. No code tests, assertion probes or manual UI exercises were written or run for this work.
