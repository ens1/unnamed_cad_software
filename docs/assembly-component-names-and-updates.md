# Component labels and controlled updates

Component references keep stable IDs. User-facing labels are a separate projection
of those references, and changing a label does not change a placement or a mate.

## Occurrence labels

- An explicit occurrence name is kept as written.
- Empty, `Part`, `Assembly`, and numbered starter names such as `Part1` use the
  meaningful definition name for display. A legacy single-part wrapper uses its
  named child when the wrapper still has a starter name.
- A newly linked project uses the source document's meaningful root name. New
  insertions get a unique occurrence name within their parent assembly.
- Repeated sibling names get a display number. Nested labels include the full
  occurrence route, separated by `›`. Adjacent identical single-part wrapper
  labels are shown once. Complete ID paths remain in the semantic data.
- Tree rows, geometry picking, selected-occurrence overlays, mate reference
  choices, and interference pairs use the same naming rules. A saved `Part1`
  source name is not silently rewritten, including in pinned libraries.

`cad_core::occurrence_names` owns this projection independently of the renderer.
Evaluated leaf occurrences carry these labels, so other geometry reference
pickers receive the same names.

## Name-only edits

A `PutInstance` command that changes only the name can reuse the current exact,
valid evaluation. The command still validates the document and creates a draft;
Apply commits it through the normal undo transaction, and Cancel retains the
starting checkpoint. The evaluation receives the new request identity and fresh
occurrence/inspection labels. A fresh draft discards an obsolete committed
operation-difference preview.

An existing draft keeps its current validated difference preview. A fresh
metadata-only draft does not request a new geometry-difference job. Production
table preferences use the same path when every other model setting is unchanged;
they still validate their table options and source document.

Pending, invalid, or stale evaluations cannot take this path. Any placement,
configuration, grounding, finish, definition, or expression change still requires
evaluation. This optimization does not change ordinary Undo/Redo rebuild behavior.

## Linked component updates

Each source group keeps its name, source project, pinned revision, available
revision, and Check/Locate/Preview actions together. Adding a different source is
in a separate collapsible section. A check records availability for the exact
current pin; it does not move the pin.

The reviewed update exposes an engineering summary: named parts, materials,
finishes, parameters, features, sketch relations, assembly members, mates, and
configurations. Affected references retain complete occurrence paths. Exact
hashes, identifiers and JSON differences remain under Advanced Source. The same
summary rows and grouping are published as semantic elements for custom themes.

Preview, current evaluation, Apply, Cancel, and stale-result checks retain the
existing controlled-update transaction. Existing explicit occurrence names are
not changed when a linked definition is updated.

## Verification scope

This change is verified by compilation, formatting and linting. No code tests or
manual UI checks are run for this implementation. The saved release-review
assembly remains the interactive acceptance case for repeated linked `Part1`
wrappers, a renamed pedestal, inspection pairs, and a steel-frame source update.
