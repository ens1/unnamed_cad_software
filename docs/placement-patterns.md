# Sketch Driven and Table Driven Patterns

TCAD's `tcad.placement-pattern` contract, version 1, records explicit placements of an existing native solid body output. An output may contain several solids. Every active placement copies that complete set with one proper rigid transform; it does not tessellate, deform, fuse, or approximate the source geometry. New source solids still begin with sketches.

## Authoring and editing

**Sketch Driven Pattern** uses individually selected native sketch points. Invoke it while editing a sketch to retain the points selected when Finish Sketch commits the current validated sketch. The property manager also lets the user explicitly add or replace a target from preceding unsuppressed sketch points. If no points were selected, the manager starts with one existing draft target, preferring the selected sketch or a point-only sketch, and identifies it in the status message and expanded target field for review or replacement. This feature remains uncommitted until the user applies the current validated draft. A new point is never added to an existing pattern merely because it appears in the sketch. Without a suitable sketch, the command starts a position sketch in the same model viewport.

**Table Driven Pattern** uses authored coordinate-expression rows or explicit linked point references. The docked point selector supports the shared datum, sketch point, origin, and native model-edge point sources. X/Y/Z use the document's f64 length-expression evaluator; rotation uses its angle evaluator. Rows expose Skip, move earlier/later, and Remove controls, with a bounded page of scrollable property fields for large tables.

Both modes expose Bodies to Pattern, Seed Point, Rotation Axis, and Keep Source Body. Each target locates the seed point. For source point `p`, resolved seed `s`, target `t`, and rotation `R`, the placement is `R * (p - s) + t`. The associative axis supplies the rotation direction; its origin does not change this seed-centered placement rule. With zero rotation, this is the translation `p + t - s`.

Keeping the source retains its original solids once. An active, zero-rotation target exactly at the seed reuses that retained source and consumes no second body set. Two active rows with the same exact rigid transform are rejected; skip or move one row. Distinct placements remain distinct bodies, including overlapping bodies. Merge, feature patterns, and component patterns are separate operations.

All property edits update the shared source draft and persistent geometry worker preview. Apply waits for the current validated evaluation and commits one undoable change. Cancel restores the feature's starting checkpoint. Deferred Sketch Driven Pattern requests retain document/part IDs and the final explicit point selection; changing documents, canceling the sketch, or a mismatched part cannot apply a pending pattern to a different model. Pinned library definitions must be made unique before authoring.

## CSV tables

Coordinate tables use this header:

```csv
key,x,y,z,angle,skipped
station-A,25 mm,0 mm,0 mm,0 deg,false
station-B,50 mm,0 mm,0 mm,90 deg,true
```

The `key` is the placement's saved identity. It must be nonempty, unique within the feature, at most 256 bytes, and contain no control characters. Reordering rows, moving a target, changing its angle, or toggling Skip leaves that key intact. Import replaces the feature's draft records and retains the supplied keys. Preserve existing keys when editing an exported table so retained topology references continue to refer to the same placements. Removing a row deletes its identity; adding a row in the native editor creates a fresh ID.

X/Y/Z and angle fields preserve authored expressions, including explicit units, parameter references, and surrounding whitespace. Keys preserve their exact whitespace as well. CSV quoting supports embedded commas, quotes, and line breaks; malformed rows, duplicate keys, empty quantity expressions, and invalid Skip values are reported before replacing the draft. Empty physical lines are ignored, while explicit empty records and delimiter-only rows fail validation. Skip accepts `true`/`false` or `1`/`0`. UTF-8 input is bounded to 1 MiB and 10000 records. Native source save preserves all linked point sources. Coordinate CSV export rejects linked targets rather than inventing coordinates or silently discarding associativity.

## Exact geometry and identities

The application-owned definition contains `body`, `seed`, `axis`, and `placements[]`, where each record contains `id`, `target`, `angle`, and `skipped`. Source dependencies include all saved records, including skipped reservations. Native topology identities use the feature owner, the authored placement ID, and the retained source face or edge construction ancestry. Coordinates and row numbers are not identity keys. Reordering a row does not rename its copied faces or edges. Semantic element IDs encode the exact key bytes in a dedicated record namespace, so punctuation and reserved field names in authored keys cannot collide with controls. Library import hashes namespaced record identities to fixed UUID strings, preserving the native key bound; Make Unique retains each record's local identity.

The kernel collects one absolute occurrence of each source face and transforms that aggregate exact topology. It then reconstructs every source solid with the original shell and face orientations. Shared source faces, edges, and vertices therefore remain shared within each placement's body set. Each resulting solid must pass oriented closed B-rep and actual boundary/carrier validation. Missing native ancestry, distinct source entities sharing ambiguous ancestry, and failed geometry validation are explicit errors.

Resolved points and transform coefficients must be finite, the axis direction must be nonzero, and rotation is bounded to one million turns before normalization. The output is bounded to 10000 materialized solids and two million native topology entities across retained/copied body sets. Identity-source reuse is accounted for before this resource guard. Reducing active placements or source complexity resolves an over-budget request.

## Current bounds

Sketch mode accepts saved native sketch-point sources; it does not infer a changing set from all points or from rectangle corners. Table mode accepts coordinate expressions and the existing resolvable point-source family. Implicit intersection edges remain unavailable as native parameter points when the shared resolver cannot establish an exact retained carrier. Missing, suppressed, or stale active sources fail explicitly. A skipped record retains its source intent and identity, and is evaluated as geometry when unskipped.

These operations pattern complete solid body outputs. Per-face deformation, variable scale, feature reexecution, automatic merging, and component-level placement are not implemented by this contract. CSV stores coordinates and quantity expressions, while the native format preserves linked point associativity. Compiler and lint validation are performed as part of the integrated build; interactive acceptance belongs to the user, and no code tests, probes, or manual UI checks are added.
