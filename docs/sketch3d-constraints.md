# Spatial sketch constraints and solver contract

`cad_core::sketch3d::Definition` v1 stores absolute f64 XYZ controls in
millimetres, stable source identities, curve definitions, construction flags,
relations and a workplane. `Operation::Sketch3d` embeds that source. Its
application-owned types contain no renderer or native kernel objects. Ordinary
planar sketches retain their existing source and solver contracts.

Lines, three-point arcs, oriented circles, Bézier curves, clamped positive-weight
NURBS and supported interpolating splines use the same authored controls during
constraint solving and native curve construction. A circle's center is its
absolute source point; its plane supplies fixed U/V orientation. The plane
origin is not a second center. Rational domains must have finite positive width.
Interpolating splines retain their fit points and explicit parameterization;
singular or poorly conditioned fits are rejected.

Relations support fixed XYZ coordinates, axis coordinates, coincidence,
point distance, signed axis distance, distance to a straight supporting line,
line/arc/circle length, arc/circle radius, straight-member angles, axis alignment,
parallelism, perpendicularity, equal lengths, point-on-line, workplanes,
coplanarity, point-on-curve and regular curve tangency. A point-on-curve relation
may use a fixed normalized parameter or a free bounded parameter. Tangency uses
explicit contact parameters, positional equality and parallel regular first
derivatives. Interior spline contacts without a unique first derivative receive
an unsupported diagnostic rather than a guessed one-sided tangent.

Exactly zero point distance means three coordinate equality equations.
Exactly zero point-to-line distance uses its perpendicular displacement vector.
These forms preserve the local rank of the intended coincidence constraints at
zero separation. Positive distances use their scalar length equations. A full
circle's driving radius replaces its effective radius expression directly.
A driving circumference derives the associative expression `value / (2 * pi)`
for its radius, preserving length units and parameter dependencies. Circle
orientation and radius are not phantom point-coordinate freedoms. A full circle
may have one driving size dimension, either radius or circumference; additional
driving size dimensions are rejected. Driven size measurements may coexist.

Equal Radius between full circles is stored by the existing `EqualLength`
relation: equal circumferences imply equal radii. The native preparation stage
resolves the connected circle group into one associative radius expression
before positional solving. Without a driving size dimension, the first selected
reference circle supplies that expression; relation direction is `first` to
`second`, so the reference does not depend on sorted source identities. A single
driving radius or circumference anywhere in the group takes precedence and
drives every circle while preserving its parameter expression. Authored radius
fields remain source inputs; prepared effective radii belong to evaluated output.
The editor does not bake those effective expressions into the original members.
Multiple driving size dimensions, circular reference chains and competing
undriven reference roots receive explicit diagnostics. Duplicate directed links
and directed transitive links do not create additional reference ownership.
Driven equalities measure the current size difference without propagating radii.
The application-owned `circle_size_owner` query exposes the chosen reference,
optional driving constraint, radius-versus-circumference mode and effective
radius expression. Its batch counterpart `circle_size_owners` resolves every
full circle in one validation and graph pass; presentation adapters reuse that
map while projecting several selected fields. Editors use that ownership to
disable dormant follower seed fields and identify the editable reference or
size dimension. Joining existing
undriven equality groups attaches their reference circles; selecting an already
connected group again preserves its existing size ownership.

Equal Length between straight members and three-point arcs retains genuine
positional equations. An arc's length depends on its radius and sweep, so this
relation does not mean Equal Radius for arcs. Spline-length equalities remain
unsupported rather than introducing approximate arc-length freedoms.

Driven relations report current measurements and residuals without constraining
the solve. Length measurements use millimetres and angle measurements use
radians. Spline arc-length constraints and freely solved circle orientation are
currently unsupported. The report's freedom count and point-motion directions
describe local infinitesimal freedom, not finite reachability or a minimal
conflict explanation.

## Native endpoint identity

Shared topology follows source identity or explicit equality intent rather than
coordinate proximity. The endpoint graph includes source points and a logical
full-circle seam at normalized parameter zero/one. Nondriven Coincident and
exactly zero Distance relations join source points. Tangent joins endpoints only
when both contact parameters evaluate to exactly zero or one. Interior or nearly
terminal contacts never create endpoint aliases implicitly.

Finalization normalizes proven source-point aliases from a single position
snapshot, then rechecks actual endpoint positions against their canonical
endpoint within 1e-7 mm. Circle carriers are not translated to fabricate shared
seams. All driving relations are rechecked after normalization and the reported
maximum residual reflects that final geometry. The native adapter consumes the
same logical endpoint graph and retains its carrier/boundary checks.

## Preview, dragging and limits

Normal solve and constraint-aware dragging use the shared native damped LM/SVD
engine. Drag targets are transient worker requests, not persisted sketch source.
Cursor goals are soft; engineering equations are projected and validated before
the solved candidate is published. Failure leaves the caller's source unchanged.
The current worker identity, editor identity and request sequence identify the
result that may update a draft. Applying still requires a current valid model
evaluation.

A solved drag may change both point seeds and free point-on-curve parameter
seeds. Draft integration must preserve matching constraint identities and
point/member targets while baking those seeds. Active configuration overrides
must receive the current seed edit or expose why that edit is blocked; updating
an overridden base seed alone does not change configured geometry.

Source limits are 256 points, 256 members and 512 constraints, with bounded
expression text, spline controls and finite coordinates. Solves bound variable
and equation counts and share an explicit work budget. These are runtime
admission limits rather than claims that every possible constraint system is
solvable. Native construction separately rejects collapsed or invalid geometry.

Compilation, formatting and strict linting are the implementation checks. No
tests, probes or interactive acceptance runs were performed.
