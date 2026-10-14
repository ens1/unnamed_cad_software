# Native kernel foundation — 2026-10-14

This work changes actual geometry paths in TCAD and the public Monstertruck
dependency fork. It does not establish completion of the mechanical CAD
baseline. Source review, compilation, formatting and linting are the allowed
verification methods for this work; interactive acceptance remains with the
user. No code tests, assertion probes or manual smoke checks were performed.

## Intersection and trimming

Classic Boolean intersections retain paired bounded UV continuation traces and
the producing native face identities. Curve subdivision, inversion, transforms,
conversion and content fingerprints retain that branch information. Division
uses native branch evaluation in the corresponding producing face's parameter
frame. It does not independently choose an unrelated projection for each face.
Unresolved UV junctions and winding are reported explicitly.

For native affine planes and clamped positive polynomial/rational spline
carriers, discovered degree-one guide spans now pass a bounded interval
continuation proof. Its parameterized contractions establish a unique local
root inside each admitted UV tube and bound one-sided joins to the saved
modeling residual. Retained branch halfspaces use complete native scalar
residual bounds, with refinement when a whole span cannot be admitted. Native projection must stay inside those tubes. TCAD
reconstructs the guards from the current geometry before admission, meshing,
inspection and export; transforms invalidate the transient guard cache.
Tubes are excluded from serialized geometry and geometric content hashes.
They do not inflate saved geometry/provenance records or change feature
identity when an admission cache is refreshed. A process-local evidence cache
retains at most 128 entries and 4 MiB of tube data, keyed by the complete
serialized curve/carrier definitions and saved residual. Oversized key sources
bypass this optional cache, and shared endpoints are checked even on cache hits.

This establishes local continuation of a discovered branch, not complete
intersection discovery, global branch injectivity or absence of other loops.
Angular analytic carriers remain on their existing native checks. Unresolved
supported-carrier proofs fail explicitly; no display guide becomes a boundary.

The paired traces are numerical search hints, not exact pcurves. Only an exact
boundary provider populates face-local trim storage. Modeling no longer accepts
five matching surface samples as carrier identity. TCAD's central mesh path
uses the native trimmed-shell tessellator where exact per-face boundaries are
available, with finite range, carrier and shared-endpoint admission.

The topology layer preserves face-local trims through inversion and fallible
mapping. Trimmed compression retains stable vertex/edge/face IDs, allocator
state and attributes. Extraction validates trim layouts, reference indices and
identity metadata rather than indexing malformed arrays or returning unchecked
solids. These are dependency contracts; TCAD's primary evaluated Shape still
contains ordinary native bodies/shells and reconstructs available exact trims
at admission, meshing and export. A complete persistent per-edge-use migration,
including imported seam charts, remains work.

## Connected native modeling

Complete tangent line/arc cap contours on finite recognized extrusion stock
construct fillet/chamfer strips together with one shared junction/rail graph.
Uniform shells can recover that rounded-rim stock, offset its supported native
carriers, reconstruct cap/wall intersections and close supported planar
openings. Complete collar, focal and axial bounds protect against overlaps and
collapse. These are bounded mechanical families; arbitrary mixed-carrier
networks, partial contour junctions and variable-radius corner transitions are
not implemented by this path. See
[the supported construction families](connected-analytic-rims-and-shells-2026-10-14.md).

Lofts and positive rational surface patches retain the fast fixed-projection
proof and add a local chart atlas with complete chart-pair separation. Loft
skins check pairs across all section intervals, including shared curved seams
and isolated shared vertices. Nonparallel solid sections require complete skin
and exact end-cap separation; tolerance-flat caps cannot borrow that proof.
Failure to certify within a bounded work budget is reported as unresolved,
not as proof that the user's surface is folded. See
[loft construction and limits](loft-geometry.md).

## Native cut parameters

B-spline curve and surface splits preserve the requested stored parameter
instead of snapping it by a model-length tolerance. New fallible split APIs
leave the source unchanged on failure. Strict knot multiplicity and direct
narrow-span ratios apply through insertion, value evaluation and explicit
derivative construction. Endpoint pieces retain the actual boundary, including
unclamped carriers; malformed interior knot blocks are rejected while native
derivative endpoint padding stays compatible. Legacy sectional trimming also
uses the proper axis and exact active-domain comparisons.

This removes parameter relocation, not coefficient rounding. Native residual
admission remains necessary, and extreme derivative conditioning can still be
unsupported. The broader legacy sectional synchronization helper is not certified.

## Numerical policy

Saved modeling residual, Boolean discovery resolution and disposable display
chord tolerance are separate. Native construction/cache identity uses saved
modeling policy; viewport refinement cannot change the modeled solid. Worker
protocol, evaluation responses and release manifests carry the policy used.
Legacy combined tolerance migrates only into display quality. Existing native
constructors can enforce stricter numerical limits. See
[the tolerance contract](modeling-tolerance-policy-2026-10-14.md).

## STEP fidelity

Declared STEP 3D authority remains declared 3D geometry; associated surfaces
are no longer converted into an instruction to solve a fresh intersection.
Parameter-curve conversion uses known native UV mappings, including the
spherical longitude/latitude mapping in both processor orientations. Spherical
derivatives, inverse-search hints and parameter subdivisions use the same frame.
Unsupported parameter-master conversion remains an explicit refusal.

Export materializes supported native parameter boundaries algebraically with a
complete numerical residual bound. Implicit intersections require agreement
with both stored exact carrier boundaries and the selected branch halfspaces;
the search guide is never substituted as exported 3D geometry. Unsupported
representations fail before atomic file replacement. Formatter failures return
an application error rather than panicking through Display::to_string.

Exported exact PCURVEs reference the adjacent face's emitted carrier entity.
They are included only when the surface formatter preserves its native UV
frame; otherwise the optional pcurve is omitted while the certified 3D curve
and carrier association remain. The exported assembly uncertainty records the
saved modeling residual in millimeters. General implicit-boundary interchange,
periodic seam-chart preservation and arbitrary parameter-master STEP import
remain incomplete.

## Remaining kernel work

- Complete native intersection discovery and Boolean inside/outside classification,
  including undiscovered loops, tangencies and curved contact arrangements. A
  certificate for a discovered branch does not prove that all branches were found.
- Persistent per-edge-use trim and periodic seam charts throughout TCAD's Shape,
  import, transform, edit and export paths. This checkpoint reconstructs available
  exact trims and retains their metadata; it is not that full representation migration.
- General mixed-carrier blend junctions, variable-radius rounds, face/full-round
  modes, arbitrary freeform offsets and wall-transition reconstruction.
- Faithful exchange of general implicit boundaries and unsupported parameter-master
  STEP geometry. Search guides and fitted display curves cannot replace them.

These are explicit engineering gaps. The bounded construction families described
above expand actual supported modeling but do not close the entire kernel baseline.

## Verification record

Monstertruck is pinned to `6dba41312cccaa67ae146ef869b8380ddf8411d8` on the public
`ens1/monstertruck` fork. TCAD's backend fingerprint is
`monstertruck/0.4.1+6dba41312ccc;adapter/45;solver/tcad-native-lm/18`. Earlier geometry caches are invalidated.

Completed against the pinned dependency:

- `cargo check --workspace`.
- `cargo fmt --all -- --check`.
- `cargo clippy --workspace --locked -- -D warnings`.
- Fork Rust 1.99.0 strict library Clippy for geometry, modeling, solid,
  topology, IO and healing, plus targeted nightly formatting of all 43 changed
  Rust files and whitespace checks.

The existing dependency future-incompatibility notice is `proc-macro-error2`
2.0.1. `scripts/package-macos.sh` completed its locked workspace build,
Info.plist validation, nested executable signing and deep strict bundle signature
verification. The refreshed artifact is `dist/TCAD.app`, containing distinct
`TCAD`, `tcad-worker` and `tcad-cli` executables. Application implementation
checkpoint: `dd479eb`; the public dependency comprises nine commits after the
previous `fc230685` pin.
No tests, runtime geometry probes, examples or interactive checks were run.
Compilation and source review do not establish that an authored part now
succeeds or that a proof admits every valid geometry.
