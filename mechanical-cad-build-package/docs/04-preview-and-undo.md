# Preview, Apply, Cancel, and undo

## State ownership

Maintain a committed document revision and a separate draft transaction. Editing a radius field, scrub scale, graph input, or viewport handle changes the same draft definition.

Expose draft values, text, status, diagnostics, and Apply/Cancel through theme-agnostic semantic bindings. User layouts can move, duplicate, or restyle controls; every copy addresses this same transaction. Switching layout/theme preserves the draft and never commits or cancels it implicitly.

A preview is a real evaluation of that draft. The UI may offer a fast approximation only if it is explicitly marked and replaced by the validated result before Apply.

## Request identity

Every evaluation envelope includes:

- Protocol version.
- Worker process generation.
- Base committed document revision.
- Draft ID, or a committed-evaluation designation.
- Monotonically increasing draft edit sequence.
- Source/dependency fingerprint and kernel/solver implementation fingerprint.

Each result repeats the identity. Accept it only if every field still matches the active request. A Cancel or superseding edit invalidates the previous draft/request immediately, even if the worker continues computing.

## State transitions

| State/event | Behavior |
|---|---|
| Begin edit | Snapshot base revision; create draft; retain committed geometry |
| Change parameter | Validate type/units; update draft; increment edit sequence |
| Request preview | Evaluate draft asynchronously; mark pending |
| New result | Check identity first, then validity; display current preview |
| Evaluation fails | Keep committed model and draft values; show actionable diagnostic |
| Cancel | Invalidate draft and requests; restore committed presentation |
| Apply while pending | Wait for/request the current validated evaluation; never apply an older result |
| Apply current valid draft | Check base revision; atomically commit all linked definitions as one undo entry |
| Base revision changed | Reject the commit or perform an explicit validated rebase |
| Undo/redo | Restore domain definitions; invalidate incompatible jobs; re-evaluate |
| Worker restarts | Invalidate its handles/results; rebuild from definitions |

Ending a gesture, losing focus, releasing a scrub, or leaving an isolated view does not implicitly commit.

## What is previewed

Show changes at their actual part/assembly positions. For a shared support definition, preview every affected instance. For a spacing parameter, include affected placements and derived shaft/hole geometry.

Selection outlines, neighboring ghosts, and proposed material use different labels/line treatments or hatching. A user can compare Original and Ghost while the draft remains active.

Keep opaque committed geometry visible while a preview is pending. Never replace it with an invalid mesh or erase it because the new job failed.

## Undo transaction

The undo record captures authoritative before/after definitions and relevant editing metadata, not a bundle of pointers into the worker. A coordinated parameter change creates one undo entry across all affected documents.

Define presentation restoration separately: Cancel restores committed model state and the prior editing selection as appropriate; ending in-place editing restores assembly visibility and selection without undoing legitimate committed geometry changes.

## Required manual demonstrations

Manually exercise rapid edits arriving out of order; Cancel followed by a late valid result; Apply before evaluation finishes; repeated Apply; shared-instance edits; changed base revision; worker crash; failed topology resolution; undo/redo with pending jobs; fresh-process rebuilding after cache deletion; and layout/theme reload with an active draft. Record observed behavior and diagnostics. Do not write automated code tests or assertion scripts for these scenarios.
