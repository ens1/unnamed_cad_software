# Native spatial sketch geometry and consumers

Spatial sketch contract `tcad.sketch3d` v1 saves absolute f64 XYZ millimetre
controls, stable member identities, native constraint intent and a workplane.
`Operation::Sketch3d` embeds that source rather than reusing a planar sketch with
discarded Z coordinates. The initial geometry backend is
`monstertruck/0.4.1+fc2306852a46;adapter/42;solver/tcad-native-lm/17`.

The geometry adapter solves through the application-owned constraint-solver
contract, then constructs native wires for lines, three-point spatial circular
arcs, oriented circles, Bézier curves, clamped positive-weight NURBS and supported
interpolating splines. Circles use their absolute center control; saved plane U/V
axes supply orientation, without a second center from the plane origin. Closed
rational curves split at a native parameter station when required for valid
closed topology. Interpolating splines retain fit-point intent; conditioning and
fit-error limits remain part of source validation.

Native vertices share authored point identities, solved coincidence/zero-distance
relations, and tangent contacts explicitly located at both carriers' endpoints.
Full circles use a logical periodic seam; interior contacts do not invent a
native split. Shared seam vertices retain the existing circle carriers and pass
the declared boundary tolerance. Unrelated nearby controls are not automatically
healed together.
Branches split into maximal native chains for display; choosing a whole sweep
or pattern path requires one nonbranching chain, or an explicitly selected member.
Construction members remain available as explicitly selected reference paths.

## Consumers

Existing application-owned source kinds now resolve spatial inputs explicitly:

- `CurveSource::SketchMember` selects the member's actual native pieces for
  Composite Curve, with native source order and construction ancestry retained.
- `PointSource::SketchPoint` resolves an absolute solved XYZ control for datum
  points, Curve Through Points, coordinate-system construction and other consumers
  of that same point contract.
- `AxisSource::SketchLine` uses the actual spatial line direction.
  `AxisSource::SketchCircle` uses the exact supported circular carrier's axis.
- Sweep and Curve Pattern accept a selected spatial member or one complete
  nonbranching material path. An authored terminal/start endpoint can determine
  path direction. The existing native sweep-frame, regularity, spacing and
  self-intersection limits still apply.
- Structural members accept selected spatial **straight lines**, with full XYZ
  stock, existing locating points/roll/offsets, end offsets and native planar
  trims. They are not planar approximations of the layout.

Spatial sketches remain wires and controls. They have zero solid/surface body
counts and contribute no volume, mass or solid manufacturing rows. They do not
implicitly become planar material regions, extrudable faces or surface fills.
General spatial arc/spline structural stock, general nonplanar mate-path intent
and automatic planar-section conversion remain unsupported by this addition.

## Structural section frames and joints

The saved spatial workplane supplies the section-clocking reference. Normally,
the section X direction is the workplane normal crossed with the member tangent.
For a tangent parallel to that normal, the saved workplane U direction supplies
the transverse reference. Section rotation then acts about the true member axis;
the existing locating point and signed section X/Y offsets use that rotated
frame. No world-coordinate flattening occurs.

Independent butt members retain native tangent-normal end caps. Existing explicit
trim planes, shortening/extension and end-cap construction operate on the actual
spatial prism. Automatic miters still need exactly two incident members.

Spatial miter admission preserves the existing planar-clocked case when both
incident axes are perpendicular to the workplane normal. Additional section
proofs admit centered concentric circular stock, which is rotation independent,
and centered authored rectangular boundary loops. Rectangles allow reflected
section-axis sign changes; equal-sided loops additionally allow X/Y swaps.
Every rectangular exterior/hole boundary must satisfy the source proof. The
reflected member frames must match the section's admitted symmetries within the
existing angular guard. Offset, asymmetric or incompatible stock receives an
explicit diagnostic directing the user to Independent Butt, explicit trim planes
or a compatible workplane. No junction clocking is inferred from a display mesh.

## Evaluation, presentation and bounds

Native edges retain construction references containing the stable spatial
member ID and native piece index. Solved source, solve report, native-derived
display polylines and diagnostics cross the adapter as `sketch3d::Evaluated`;
kernel vertices/edges never enter authoritative saved source. Constraint residuals
and local point-freedom directions come from the same current source and solver
contract in the persistent geometry worker. Optional analysis failures remain
explicit diagnostics; they do not fabricate locked/free states or invalidate
otherwise valid model source.

The evaluator publishes source/configuration-qualified spatial output alongside
ordinary model data. A current successful solve is cached before native curve
construction. If subsequent native geometry fails, that solved input and its
diagnostic remain available for editing. Failed solving does not invent a solved
result. Caches clear for each evaluation and each rebuilt owner, and normal
worker identity guards reject stale proposals. Partial input display never
authorizes Apply of failed geometry.

Source admission permits at most 256 controls, 256 members and 512 constraints.
Native display construction has a shared one-million-point budget per sketch;
the existing bounded curve-subdivision rules also apply. Polylines are disposable
display data, never length, topology, clearance or mass authority.

Compilation, formatting and strict linting are the implementation checks. No
tests, probes or interactive acceptance runs were performed.
