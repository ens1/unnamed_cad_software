# Sketch contour inspection

The active sketch editor exposes compact **Inspect** actions when its current
material members contain a branched contour, duplicate straight-line endpoints,
collapsed line, or an ambiguous cluster of at least three straight members at an
exactly coincident endpoint. The default block follows the active drawing and
member inputs and precedes the folded secondary editors. It is absent when no
actionable finding is available.

Inspection selects the finding's authored `SketchEntity::Member` identities,
using the same viewport highlighting as ordinary member selection. It does not
repair the sketch, solve, request an evaluation, change constraints or source,
create an undo entry, or alter a pending dimension checkpoint. Finish an active
drawing, handle drag, modifier, dimension, or confirmation gesture before using
an inspection action. Members can then be edited, deleted or changed to
construction through the existing sketch commands.

The analysis reads `sketch::sketch_definition` from the current editor. It
excludes construction and suppressed members. Branched contours come from the
application-owned `sketch_topology::analyze` connectivity contract, including
authored connections, pattern provenance and its stated 0.000001 mm isolated
endpoint healing. **Shared section endpoints** therefore means common section
vertex identities, without claiming exact geometric duplication.

**Duplicate line endpoints** uses exact finite authored f64 endpoint locations
in either direction, treating signed zero as the same location. It introduces
no additional proximity tolerance. Separate point identities at the same exact
location are also inspected when three or more distinct material lines meet
there; such ambiguous clusters are intentionally not automatically joined.
Nonfinite controls produce unavailable analysis rather than an inferred result.

These findings describe source connectivity and straight-line endpoint
relationships. They do not certify contour nonintersection, positive area,
hole nesting or native solid validity. Absence of findings is not an acceptance
of the sketch for a solid operation; current native evaluation remains the
authority for Apply.

Analysis is cached by document, definition, feature, sketch, editor token and a
source fingerprint. Borrowed source is serialized into a streaming SHA-256
writer with a 256 KiB limit before any analysis snapshot is cloned. Synchronous
inspection is bounded to 128 points, 128 members and 256 constraints to keep
editing responsive. Larger sources publish a `limited` summary without
actionable findings; their normal sketch and native evaluation remain available.
Every action revalidates its complete current scope, fingerprint, issue kind
and exact member identities. Actions from a prior source or editor are rejected.

Custom layouts can render the `sketch-issues` component or repeat semantic
elements with `role: sketch-issue`. The summary role is
`sketch-inspection-summary`; `owner` and relationships identify the current
sketch. Stable issue identity is derived from its kind and sorted authored
members. IDs and member details remain semantic data rather than native button
captions. Both roles accept user styling, including compact text and target
dimensions. For example:

```json
{
  "id": "my-sketch-issues",
  "role": "semantic-action-collection",
  "layout": "column",
  "overflow": "auto",
  "repeat": { "role": "sketch-issue", "owner": "my-sketch-id" },
  "template": { "role": "sketch-issue", "widget": "action" }
}
```

Semantic action `sketch.inspect-issue` carries the versioned
`tcad.sketch-contour-inspection` contract. The descriptors explicitly declare
`source_mutation: false` and `native_validation: false`.

Validation for this change is compilation, formatting and linting. No code
tests, runtime probes or interactive acceptance checks are introduced or run.
