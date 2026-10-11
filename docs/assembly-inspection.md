# Assembly interference and boundary distance

`tcad.assembly_inspection` contract 1 stores inspection intent on the owning assembly. Its report is disposable evaluation data. Each target saves the complete root-relative leaf occurrence path and its exact expected leaf configuration; equal definition names or repeated component instances are never interchangeable. Missing, suppressed, ambiguous or configuration-changed targets remain explicit inactive entries. Reordering the selected scope preserves an unordered pair's saved result identity, while first/second witnesses retain report order.

An inspection may follow every owning assembly configuration or a particular named configuration. Leaf configuration identities remain explicit in both modes. The docked editor can bind to the current named configuration or remove that binding. Editing captures the current root configuration, and switching it requires finishing or cancelling that edit. The initial scope contains up to 64 active leaves, with explicit notice when a larger assembly requires scope selection. Up to 32 inspections can be saved per assembly. Open the owning assembly as the document root to edit or inspect its report.

## Native interference

Interference uses the actual evaluated configured native solids and mate-solved rigid, right-handed occurrence placements. Exploded views and cached history presentations never affect the calculation. Open surfaces participate in boundary distance but do not establish volumetric interference.

For a selected body pair, conservative native trimmed-carrier bounds or admitted positive explicit rational control hulls can establish axis separation with a margin based on the authored Boolean tolerance and coordinate magnitude. Otherwise the native Boolean common is constructed at the saved tolerance and checked as a closed, oriented B-rep with consistent boundaries. Nonempty validated common results report native interference and zero engineering clearance. The result counts body-pair common outputs, rather than pretending to count union-connected overlap regions.

An empty common result does not certify separation or settle coincident/tangent contact. Backend errors, invalid common results, unsupported carrier preflight and exhausted budgets remain explicit uncertainty. These are f64 native construction and validation results, with no claim of exact-arithmetic classification or general coincident/tangent robustness.

An optional common-volume estimate is available only for a single complete body-pair common result with planar faces and straight boundary edges. A shifted-origin compensated signed-boundary integral handles oriented outer loops, holes and cavities without invoking an unbounded tessellator. It has no numerical error certificate. Curved common results omit volume. Multiple body-pair common results may overlap, so their volumes are not summed into a misleading union volume.

## Approximate boundary distance

Minimum boundary distance uses the currently evaluated f64 boundary triangles after applying assembled occurrence placements. A bounded spatial hierarchy searches vertex/triangle, edge/edge and edge/triangle intersection candidates. The returned value and its two witnesses are explicitly approximate; no native tessellation-deviation or numerical error bound is available. The requested clearance comparison describes this boundary estimate and is not a native clearance certificate.

Containment can have a positive boundary distance while native volumetric interference establishes zero engineering clearance. Reports keep those two quantities distinct. Faces in witness records retain application-owned output owners and construction references. Derived mesh coordinates do not become authoritative saved model geometry.

Highlight Pair restores the latest assembled presentation, removes isolation/ghost filters and clears incompatible picks before showing both occurrence bounds and the optional approximate witness segment. Select A/B selects the corresponding current evaluated face with a checked vertex from that face's index range. It does not turn a witness estimate or a bounding-box center into a fictitious exact source point. A result action requires the current source fingerprint, full session request identity, matching report contract, inspection identity and root configuration.

## Editing, persistence and work limits

The task-specific docked controls edit the shared document draft: scope, mode, configuration binding, requested clearance, native Boolean tolerance, name and suppression. Quantities support expressions in the effective owning configuration. Invalid expressions block current validation; unsupported pair geometry remains visible without rejecting the whole saved inspection. Advanced Source remains repairable after malformed JSON. Preview validates source before dispatch; Apply requires the current validated evaluation and records one undoable change. Cancel restores the starting shared document checkpoint. Definitions persist in native local files and local Git snapshots; component/library identity remapping preserves paths and configuration references.

Source validation bounds targets to 64 distinct paths of at most 64 IDs, each ID at most 256 bytes, at most 256 KiB of target source per inspection, and at most 4,096 bytes per authored quantity expression. Runtime reported coordinate values, placed native vertices and occurrence translations must be finite and within ±1e12 mm. Boolean tolerance is 0.000001–1 mm; requested clearance is finite and nonnegative.

Runtime limits are shared across all active inspections in the current assembly evaluation:

- Occurrence indexing admits at most 100,000 assembled leaves; lookup, face metadata and selected-path work share 16 million units. Duplicate full paths remain ambiguous, and incomplete indexing never establishes uniqueness.
- One occurrence admits at most 256 solid bodies. Native source and common-result admission precedes geometry copying or validation, with at most 4,096 faces and 16,384 edges per body; aggregate admission allows 32,768 faces, 131,072 edges, 1,048,576 controls and 32 million estimated predicate-work units. A carrier preflight is bounded to degree 32 and nesting depth 16; unsupported or unbounded classes are rejected.
- Cached bounds are computed once per transformed body. Native body/hull comparison permits 65,536 body pairs, 4,096 Boolean calls and one million accumulated native face pairs.
- Placed boundary data permits 500,000 triangles, 32,768 face records per occurrence, 16 million hierarchy/triangle comparisons and 131,072 pending node pairs. Cached reference metadata is limited to 32 MiB.
- Aggregate reports allow 64 MiB including occurrences, inactive targets, pairs and diagnostic/object overhead. Pair diagnostics are bounded and deduplicated. Exhaustion reports omitted work instead of asserting an all-clear result.

The current implementation does not provide certified native minimum-clearance distance, general tangent/coincident-contact classification, combined overlapping common-volume unions, hole-alignment inspection, dynamic collision sweeps or interactive acceptance evidence. Compilation and lint integration are managed by the parent task; no tests, probes or UI acceptance runs are part of this implementation work.
