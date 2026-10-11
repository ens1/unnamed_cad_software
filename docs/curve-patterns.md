# Curve Pattern

Curve Pattern is a saved solid feature that places rigid copies of a sketch-built body output along a native sketch or reference curve. It produces real, independently validated B-reps. The source definition contains no kernel handles or mesh-derived placement data. The current application-owned contract is `tcad.curve-pattern/1`.

## Authoring and editing

The Features pattern group opens a docked property manager in the existing model workspace. Select **Bodies to Pattern** and **Curve / Sketch Path**, then edit Instances, spacing, Start/End, reversal, orientation, source retention and skipped positions. Body and path choices respect the active configuration's feature suppression. A body output can contain several solids; every current solid in that output is copied.

A sketch path can be a selected material member, including a line, arc, spline, circle or ellipse, or one connected open material chain. Construction and suppressed members are excluded. A multi-member open path retains a native **Path Start Vertex**, and every explicit endpoint/member selection is checked against the current solved sketch. A closed single-member path retains its native starting phase. Multiple disconnected paths, a branching chain, a closed multi-member sketch region and stale selections require an explicit replacement or a reusable ordered reference curve.

Reference curves use their saved piece order and directions. Exact composite/through-point paths and the supported native helix/spiral carriers remain associative. Reordering/reversing a composite updates placement from the current source. The pattern does not replace an implicit intersection guide with its display curve.

With an open sketch, invoking Curve Pattern records the current member/endpoint selection, waits for validated Finish Sketch and then opens the feature manager. If no eligible path exists, it opens a path sketch and remembers the chosen body. Pending actions retain their document and part identity and are discarded by Cancel. Linked pinned definitions must be made unique before authoring. Existing feature, mate and configuration tools must be confirmed or cancelled first.

Changes use the shared source draft and persistent geometry worker. Current validation is required before OK. Superseded results cannot authorize Apply; Cancel restores the feature checkpoint, and a successful commit is one undoable operation. Saved definitions use the native document/save/export paths. Interactive acceptance remains with the user; no code tests, probes or manual UI checks were run for this implementation.

## Placement law

Instances counts path stations, including station 0. Open and partial paths include both range endpoints when the count exceeds one. A complete closed path spaces around the loop and excludes the duplicate closing station. A single station uses the chosen range start. Reversal changes traversal and tangent direction; it does not renumber the station keys.

**Equal Parameter** concatenates the increasing native parameter intervals of the retained pieces and normalizes their sum to 0–1. It is an explicit parameter law: equal steps need not have equal physical distance. Native positive-weight line/B-spline/NURBS carriers are evaluated directly. A supported analytic UV-line/revolution-line carrier is evaluated directly as well; fitting it to a spline is forbidden.

**Equal Distance (Exact)** normalizes proved path distance to 0–1. Supported laws are:

- Straight carriers whose complete rational derivative coefficients prove monotone traversal.
- Circular carriers proved by the kernel's rational polynomial circle identity. Each exact Bernstein span must stay inside an open angular half-plane and prove one forward angular traversal. Distance is radius times the signed angular magnitude.
- Native UV-line paths on a straight revolution generatrix with positive collinear radial endpoints and a rigid or uniform-scale metric. This includes cylindrical/conical helices, planar spirals and compatible reversed/composite pieces. A constant-angle or axial analytic line has its actual endpoint distance.

For a helix or spiral with radius `r(u) = r0 + d u`, angular extent `a` and axial extent `h`, the analytic speed is `sqrt(d² + h² + a² r(u)²)`. Constant-radius placement is linear in `u`. Varying-radius placement uses the closed-form integral and a monotone bracketed inverse, with a finite distance residual and a bounded position uncertainty. Stations are evaluated on the retained native carrier after inversion. No polyline length, mesh distance, quadrature samples or fitted carrier supplies spacing authority.

General spline and ellipse arc length, non-meridian analytic generatrices and sheared/nonuniformly scaled analytic distance laws are unsupported. They remain available through Equal Parameter where their native carrier is bounded. Source finite-coordinate, positive-weight and parameter-budget checks apply before materialization.

The Start/End range is measured in the selected spacing law: normalized native parameter for Equal Parameter, normalized exact distance for Equal Distance. It must satisfy `0 ≤ Start < End ≤ 1`.

## Orientation and seed position

**Fixed** translates every source solid while keeping its orientation. Sharp path junctions are usable in this mode.

**Follow Tangent** preserves the first body's authored orientation and rotates later bodies relative to that first station. The local X direction is the native tangent. The saved document-space **Frame Up X/Y/Z** direction is projected perpendicular to that tangent to define roll. Each copy uses the exact resulting rigid frame; there is no sampled transport or interpolated orientation. The up direction is explicit source intent, rather than an automatically fitted or silently changing path normal.

Complete Bernstein derivative bounds must prove a nonzero tangent/up projection across every rational span. Native analytic carriers use complete trigonometric interval bounds over their actual UV-line/revolution-line definition. Tangent directions must agree at every path junction and a closed seam. Cusps, retracing or uncertain spans, sharp corners, parallel/uncertain up directions and unsupported analytic carriers fail explicitly. These proofs currently cover the whole retained path, even when the station range uses only part of it.

Without **Align Seed Point to Path Start**, the source keeps its initial offset from the first path station. Following tangent rotates both that offset and the body. With alignment enabled, an associative model/datum/sketch/coordinate point identifies the body's anchor, which moves onto the first path station. The source's first orientation remains as designed in both modes.

**Keep Source Body** retains the original at its authored position in addition to nonidentity station copies. When station 0 is unchanged, the original occupies that station once; a coincident extra copy is omitted. Without Keep Source, every active station has a pattern-owned copy, including station 0. The result remains separate solids and does not imply a union/merge operation.

## Stable identities and bounds

Instance identity is the pattern feature owner plus its zero-based slot. Count, range, direction, spacing and geometry edits do not renumber surviving slots. Skipping a station leaves its key reserved; skips above a reduced count remain saved and reactivate when the count grows. The docked Skip Instances controls expose active skips and let the user clear dormant reservations. Skipping all active stations requires Keep Source.

Copied faces and edges combine exact source-body construction ancestry, a retained path-selection identity and the slot. Centroids, signatures, relative region areas and current station coordinates do not identify copies. A rigid constructor preserves source face/edge correspondence; missing or ambiguous current source ancestry is an error. Kept source topology retains its original references. Changing source-retention/alignment intent can retire those original references or pattern-owned station references, which must be repaired explicitly by dependent operations.

The exact copy constructor transforms one aggregate occurrence of every source face, then reconstructs each original solid boundary with its original face orientation. Faces, edges, and vertices shared across several stock solids remain shared within each station's body set. The same application-owned helper serves Sketch Driven and Table Driven Patterns; Curve Pattern additionally retains its selected path ancestry in every copied face and edge.

The source body output is consumed and replaced by the result, whether Keep Source is enabled or disabled. Generic source-output reconciliation and suppression restore that consumed branch. Count is 1–10000, paths have at most 2048 pieces, rational proofs support degrees 1–32 with at most 2048 controls per piece, and analytic frame proof has a 16384-interval limit. Materialization is capped at 10000 output solids and two million copied vertex/edge/face entities. Every affine matrix and transformed source vertex must remain finite, and every copied B-rep passes native geometry validation.

This is a body pattern. Feature-only patterns, component/assembly curve patterns, automatic merging, curve-length dimensions for general splines, automatically transported spatial roll and surface-normal following remain separate engineering gaps.

Implementation: [source contract](../crates/cad-core/src/curve_pattern.rs), [native geometry](../crates/cad-kernel/src/curve_pattern.rs), [docked editor](../crates/cad-desktop/src/curve_pattern_ui.rs).
