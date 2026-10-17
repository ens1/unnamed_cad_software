# Release review geometry follow-up — October 17, 2026

This implements the three geometry tasks separated from the
[non-kernel fixes](fixes.md). It records source changes, not a new test drive.
Implementation commit: `a0a0cbd` on `codex/modeling-workflow-reliability`.
The saved review projects have not been changed or evaluated during this work.
Their original failures remain evidence in the [review report](report.md).

## Welds on mitered members — RR-26

Straight weld roots and toe strips on convex, native line-bounded planar faces
now use a whole-footprint containment proof. This avoids solving a new nearly
parallel intersection where the weld root already coincides with a miter trim.
All strip corners must belong to the support; convexity then covers the complete
strip. Native bead construction, shell validation and source references remain
required.

The proof admits one closed, convex wire of 3–64 native straight edges. Holes,
curved trims and nonconvex faces retain the existing complete arrangement path.
Near-boundary witnesses include bounded arithmetic error. A bead that actually
extends beyond the support still requires smaller dimensions or start/end trim.
The saved tube-frame case needs user acceptance; zero trim is not automatically
valid. See [native weld contacts](../native-weld-joint-contacts.md).

## Seated component contact — RR-30

Assembly inspection now tries native support-plane separation before building a
volume Boolean. Complete admitted carriers must lie on opposite sides of the
same plane. Exact arithmetic on their stored binary64 coefficients resolves
equality without treating a small penetration as contact.

Results distinguish a strict gap, disjoint interiors with unresolved contact,
and proven touching. Touching requires an actual shared point on the native
trimmed faces; hole loops remain excluded. Only proven touching or validated
volume interference establishes zero engineering clearance. Approximate mesh
boundary distance remains a separate quantity.

Planar boundary hulls require exact membership in their own stored plane.
Tolerance-only closure, unresolved winding, revolution carriers and general
nonplanar tangencies can still require the Boolean path or remain uncertain.
The two saved pedestal/frame pairs have not been reevaluated, so this does not
claim their original failures are closed. See
[assembly inspection](../assembly-inspection.md).

## Drawing visibility — RR-18

Sheets and individual views now offer Wireframe, Hidden lines removed and
Hidden lines dashed. New sheets default to hidden lines removed. Existing
documents without a saved choice retain their previous wireframe display.
These choices persist through files, templates, undo and Git.

Native planar faces with proved straight trims use their native polygons and
holes for numerical visibility classification at model tolerance. Other faces,
including circular trims and curved surfaces, use explicitly sampled visibility
and silhouettes. Source rational edge curves remain rational when split.
Uncertain crossing intervals remain visible; exhausted work limits report an
error rather than silently emitting incomplete classified output.

Paper preview, PDF and DXF consume the same classified strokes. Hidden lines
have a separate short dash pattern; ghost/reference lines keep their existing
pattern. View placement and scale stay consistent across modes, and placement
controls use a cheaper layout calculation. Section-Z retains its separate
sampled section behavior.

Exact arbitrary curved-surface visibility and analytic silhouettes remain
incomplete. The saved curved pedestal drawing therefore still has approximate
visibility and needs user review. See [drawing visibility](../drawing-visibility.md).

## Integration and validation

The kernel adapter fingerprint advances to 46. The pinned public Monstertruck
revision remains `6dba41312cccaa67ae146ef869b8380ddf8411d8`; this change is in
TCAD's private adapters and application contracts. Exact contact predicates use
the already locked `num-bigint` 0.4.8 dependency, with its licenses retained.

- `cargo fmt --all --check`: passed.
- `cargo build --workspace --locked`: passed on the final implementation through
  the package script.
- `cargo clippy --workspace --bins --lib --locked -- -D warnings`: passed.
- `git diff --check`: passed.
- `scripts/package-macos.sh`: passed; rebuilt and signature-verified
  `/Users/tyler/Software/Personal/tcad/dist/TCAD.app` from `a0a0cbd`.
- Implementation pushed to the existing private origin branch.

No code tests, model probes, manual smoke runs or UI acceptance runs were added or run.
Recording remains stopped. The existing `proc-macro-error2` future Rust
compatibility notice is unchanged.
