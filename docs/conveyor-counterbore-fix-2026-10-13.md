# Existing-bore counterbore construction

The conveyor review's Part5 is an 80 × 60 × 20 mm straight extrusion with an
existing Ø20 mm through opening and four Ø6 mm openings authored in Sketch1.
The requested Ø20 mm bore with a Ø30 mm, 6 mm deep counterbore and 10 mm blind
depth therefore enlarges the existing opening at its entry; its narrower shank
occupies an already-empty region. The failed draft was canceled and is not
saved in the review project. The saved part definitions were inspected without
modifying or evaluating them.

## Correction

The layered native Boolean already classifies material on both sides of each
complete shared section interval. Subtraction of cutter portions inside an
existing void leaves no new material boundary. The missing step was resolving
the common circular partition before that classification: separately authored
circles can have the same carrier and quadrant vertices a few floating-point
ulps apart. Previously the circle overlap recognizer emitted both endpoint
sets, then the ordinary crossing predicate rejected a near-identical endpoint
as an uncertain new crossing.

`cad-kernel/src/join/checked.rs` now has a dedicated circular-overlap partition:

- Outward-rounded arithmetic bounds the complete circles' Hausdorff distance
  by the center separation plus radius difference, including rounding of that
  final sum. Agreement must fit within one quarter of the existing 1e-7 mm
  native membership tolerance.
- Only after that whole-carrier check may overlap endpoints reconcile within
  another quarter of the existing tolerance. Every member of an endpoint
  cluster must meet the pairwise distance bound. A bridge between distinct
  clusters or a collapsed native interval remains unsupported.
- Representatives are existing native endpoints in deterministic coordinate
  order. Other endpoints must still pass the ordinary bounded interval
  inclusion predicate. Noncoincident crossings keep their previous strict
  crossing and authored-endpoint separation rules.
- The existing layered constructor still checks native split parameters and
  residuals, complete matching subcarriers, consistent material sections,
  shared topology, shell closure and B-rep validity before publication. Tool
  and source face construction ancestry remain the reference authority.

The correction is shared by native layered drilled-hole cuts and the other
history-aware layered Boolean callers. It does not trim a drill with mesh
samples, enlarge its radius, delete an existing bore, change the authored hole
depth, or substitute a different shape. Native rational curves and native wall
surfaces still construct the output. The adapter fingerprint must advance for
cache invalidation; integration owns that shared version change.

## Scope and evidence

This addresses certified coincident circular boundaries in common-axis
straight/circular layered stock. Arbitrary curved intersections, unresolved
near-tangencies, distinct boundaries below the native tolerance and other
unsupported carriers retain explicit errors. A wholly redundant hole that
leaves no remaining cut boundary retains its existing no-material-change
diagnostic. The application-owned hole contract and saved documents do not
change.

Source validation performed on October 13, 2026:

- `cargo fmt --all`
- `cargo check -p cad-kernel` — passed.
- `cargo clippy -p cad-kernel -- -D warnings` — passed.

Cargo also reports an existing future-compatibility notice for the dependency
`proc-macro-error2` 2.0.1. No code tests, assertion probes, runtime geometry
probes, or interactive acceptance checks were written or run. Compilation and
linting do not establish the requested counterbore's runtime acceptance; that
remains for the user in the rebuilt application.
