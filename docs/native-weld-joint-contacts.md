# Native weld joints and contact recovery — October 12, 2026

The second dogfood session selected one model edge for a fillet bead. Native
validation rejected its first toe outside the inferred face's trimmed boundary.
The rejected feature was not saved, so its exact seam cannot be reconstructed
from the retained frame. This establishes a rejected contact, not a general
weld failure or an independently reproduced valid joint. The saved review model
and report remain unchanged.

The former model-edge inference considered only faces sharing the picked edge's
native identity. A multibody structural frame retains separate member topology,
so the neighboring member's support often does not share that identity. A free
convex stock edge is also a poor fillet-weld seed: its outward toe directions
leave the finite stock. Those contacts remain invalid rather than having their
tolerance widened or a cosmetic bead substituted.

## Supported joint authoring

- **Fillet:** two proven joint faces, with each complete root-to-toe strip on its
  own support. Straight joints use planar faces; circular joints use the existing
  transverse-plane/coaxial-cylinder contact construction.
- **Butt / miter:** a planar circular cap can lie on one support, or straddle two
  same-facing coplanar faces meeting at the source seam. Each half of the root
  footprint must lie on its corresponding stock. This supports a visible cap
  across planar tube-frame miter seams without requiring one finite face to
  contain both toes. It does not create a groove, bridge missing stock or fuse
  the parts.
- **Explicit source selection:** Pick Seam, Pick Support Face 1 and Pick Support
  Face 2 arm viewport selection. Ordinary geometry selection supplies native
  saved ancestry; Tab cycles overlapping candidates. Highlight Support Face
  shows its actual selectable native face beside current seam/toe guides. Stop
  Joint Picking leaves the shared draft unchanged. The same controls work for
  part and occurrence-scoped assembly welds.
- **Recovery:** Use Butt / Miter Bead is an explicit profile choice after a
  rejected fillet, followed by native reevaluation. Keep Evaluated Supports
  turns a complete inferred joint into explicit saved source references. It is
  unavailable until the current contact and bead have passed validation.
  Trim Ends by Current Leg Size / Half Width supplies an explicit conservative
  starting trim for a failed toe at a member end, then reevaluates the complete
  footprint. It does not promise that an arbitrary corner, cutout or gap becomes
  a valid joint; the exact end offsets remain independently editable.

Empty model-edge support lists now infer among native faces in the selected
seam body's scope. Carrier membership, complete trimmed root contact, joint
orientation and each footprint must pass. Only one complete supported joint may
be inferred; ambiguity requests explicit supports. Other part/assembly bodies
must be picked explicitly rather than silently adding new dependencies.
When one coplanar butt pair has proved root contact but its footprint fails at
the ends, that pair is shown as an incomplete diagnostic candidate. It still
cannot produce a deposited solid or enable Apply until every full contact passes.

## Native proof and bounds

Planar contact uses exact native line/circle arrangements. Root and toe curves,
transverse ends and support-boundary partitions prove the full footprint.
Interior cutouts are rejected even when both long contact curves remain on
material. Uncertain crossing classifications propagate Unsupported; they are
not classified as material. Cylindrical fillet contact retains its exact
rectangular angular/axial patch proof. Straight/circular continuous and
intermittent bead construction, positive native volumes, shell validation,
source ancestry and validated Apply remain required.

Inference scans at most 512 native faces and 32 contacting candidates. Aggregate
contact arrangement work is bounded across all candidates, assignments and
stitches. Exhaustion requests explicit supports or a narrower seam. An explicit
support definition is not rejected merely because its seam body's inference
inventory exceeds that bound. Separate visible deposited solids retain finish,
engineering material, cut/physical metadata and native exchange behavior.

Weld contract 2 adds the two-face coplanar butt contact and complete inference
semantics. Existing definitions remain readable. Legacy one-face butt and fillet
constructors keep their existing weld section-history purpose grammar; the
contract version is not added to their saved topology identity. Derived
metadata/caches use the new contract and backend fingerprint. Joint-guide
contract 1 is unchanged; its curves are disposable diagnostics, never authority.

Compilation and strict linting are engineering checks only. No tests, geometry
probes, manual smoke runs or interactive acceptance were performed. The saved
frame's independently chosen crossbar offsets are not automatically changed to
manufacture a joint. General transported spline beads, groove preparation,
curved junctions and support patches outside the proven native carrier grammar
remain broader kernel work outside this dogfood contact recovery.
