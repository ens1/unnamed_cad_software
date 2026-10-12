# Copy With Mates

Copy With Mates is a one-time root-assembly authoring operation. It requires a
current validated mechanical evaluation, captures the configured solved source
poses, and reserves stable identities for each new ordinary component and mate.
The shared modeling checkpoint previews the candidate document; OK requires the
current validated candidate and commits one undoable change. Cancel restores
the original document and component positions.

Select multiple ordinary direct components or whole subassembly components.
Internal mates are retained, including active saved rigid-group equations copied
as independent ordinary lock mates. External mates are explicitly listed: enable
a mate to retain it, then choose its outside target component. Same-definition
endpoints can retain their geometry source. A different definition needs an
explicit compatible current native reference, selected on that replacement.
Outside target mappings are reused for each new copy set. Auxiliary participants
and mechanical support references follow the same saved
endpoint mapping. Unmapped or incompatible enabled references reject the
candidate; they are not silently replaced with origin frames.

Count specifies new sets, excluding the originals, with bounded 1–64 copies and
4096 total inserted components. Translation and rotation use explicit expressions
and a common assembly-frame pivot. Each set receives the next transform step.
Copied fixed status is optional. Source expression-driven placements become
captured ordinary poses on new copies. Source components retain their existing
placements, equations and mates. Pattern-generated children are excluded; copy
an ordinary seed or whole subassembly instead.

New occurrences and mates are shared assembly source across configurations.
When a named placement scope is active, its initial pose overrides are saved for
the new occurrences. This does not create named-only components. Existing
configuration controls can suppress the inserted component and mate identities.
After Apply, these copies are editable ordinary occurrences rather than a
pattern-controlled feature. Rebinding does not promise the requested free pose:
the current mate solver determines the valid candidate and displays any residual
or overconstraint before Apply.

The typed editor publishes semantic fields, actions, source identity and
configuration context through the presentation adapters. User layouts and
CSS-like styling remain independent of copying intent or native geometry.
