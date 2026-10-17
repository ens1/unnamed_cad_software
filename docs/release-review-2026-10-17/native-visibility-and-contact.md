# Native curved visibility and contact — October 17, 2026

This continues the [geometry follow-up](geometry-fixes.md), specifically RR-18
(curved drawing visibility) and RR-30 (assembly contact). It records source
implementation and build checks. Saved review projects remain unchanged and
have not been evaluated during this work.

## Drawing visibility

The new default visibility calculation uses native rational/analytic surfaces,
original homogeneous curves and authoritative face-local trim boundaries.
Display triangles do not decide visibility. Outward arithmetic, bounded
inverse-map proofs, exact trim closure and native implicit silhouettes replace
the previous sampled curved-surface path for admitted geometry.

The implementation includes hole-loop parity, local self-occlusion checks,
ordinary periodic cylinder bands, end-on cylindrical views, coincident projected
boundary checks and proved tangent continuity at rational patch joins. Native
visibility events retain bounded parameter intervals; unresolved isolated
uncertainty remains an error. Original carrier definitions and model references
remain available beside the finite paper strokes.

Visibility uses modeling tolerance. Curve rendering/export has a separate
stated chord-error bound. PDF and DXF use those finite strokes; they do not
embed exact implicit silhouettes. The old sampled calculation remains an
explicit Display mesh option, selectable per sheet or view. A native failure
never silently selects it. Old wireframe choices remain wireframe.

Native scene admission, calculation, trace metadata and complete sheet output
have cumulative work/storage bounds. Native view fitting uses conservative
model bounds rather than tessellation extrema. Drawing contract 12 and template
version 4 preserve the two saved choices through undo, files and Git.

See [drawing visibility](../drawing-visibility.md) for definitions and limits.

## Assembly contact

Contact classification now uses authoritative native planar UV regions and
exact sphere/torus/revolution support predicates. Exact rational knot insertion
and quadratic Bernstein signs extend cylinder checks to shafts in through
bores, shafts seated in blind bores, flanged shafts, and admitted combinations
of a head/flange with a blind-bore floor.

Every body face must satisfy the required separating-domain proof. Coincident
faces alone do not establish separation. Exact touching also requires a shared
native boundary witness. Small construction residuals have a separate
`contact_within_model_tolerance` result with a bounded possible normal overlap;
this result never claims exact zero clearance. Any remaining body-pair
uncertainty prevents an all-clear result.

Inspection report version 3 invalidates old disposable reports without changing
saved inspection contract 1. The UI and semantic report show the contact band
separately from approximate mesh boundary distance.

See [assembly inspection](../assembly-inspection.md) for the proof domains.

## Remaining work

This does not establish universal exact visibility or contact for every B-rep.
Unproved periodic charts, singular freeform silhouettes, unproved spline
creases, general freeform tangencies, finite spherical cavities and exhausted
proof bounds remain explicit limitations. Native minimum-clearance distance,
exact section/hatching workflows and symbolic implicit-curve export remain
separate gaps. The original saved release cases still need user acceptance.

The public Monstertruck pin remains
`6dba41312cccaa67ae146ef869b8380ddf8411d8`. This work is in TCAD's private
replaceable adapters and application contracts. Adapter fingerprint 47 prevents
reuse of stale evaluated geometry. Exact rational predicates use the already
locked num-rational 0.4.2 and num-traits 0.2.19 packages; their dependency licenses
remain included.

## Validation

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --bins --lib --locked -- -D warnings`: passed.
- `git diff --check`: passed.

The full executable build and macOS package result will be recorded after
packaging this source checkpoint. No tests, geometry probes, manual smoke runs
or UI acceptance runs have been added or run. Recording remains stopped. The
existing `proc-macro-error2` future Rust compatibility notice is unchanged.
