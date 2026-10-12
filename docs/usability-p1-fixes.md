# Usability P1 implementation — 2026-10-11

This records the implementation response to the seven P1 findings from the
2026-10-11 interactive usability review. Source review, formatting, compilation,
and linting establish the implementation checkpoint. Interactive acceptance of
the fixes remains with the user; no tests or UI smoke runs were performed.

| Finding | Implementation |
| --- | --- |
| Active component and displayed context disagree | Explicit part/component navigation clears previous geometry selections, retains the full occurrence path, frames the chosen component when its evaluation is current, and uses the existing isolation/ghost controls for surrounding components. Returning to Assembly frames the complete assembly. Empty new parts frame their own origin. Context switching is unavailable during an active modeling operation. |
| Long edge provenance and clipped property fields | Native straight/circular edge summaries use model units; Focus/Remove are inline and Radius/chamfer distance precede the edge list. Full provenance stays in semantic data and Advanced Source. CSS-sized captions and editors share bounded panel space, and narrow rows stack. |
| Face sketch starts off-screen | Normal To centers and frames the evaluated face bounds in the actual occurrence placement, without changing the sketch plane, origin, or saved references. |
| Validated mate causes unexpected component movement | Assembly iteration uses a physical pose metric and rejects near-null singular directions. Unfixed legacy assemblies have no arbitrary UUID-first anchor; newly inserted first components are explicitly fixed. Placement seeds, validated solved pose/DOF, and signed mate gap are separate presentation data. |
| Basic sheet stock fails provenance | Base Flange/Tab and stock cuts retain exact member/piece extrusion ancestry for caps, sidewalls, and shared boundaries. Rebuilding stock preserves each feature's producer. Ambiguous native references still fail validation. |
| Auto drawing dimensions and circular picking fail | Parallel straight edge picks infer horizontal/vertical or perpendicular separation with two exact associative curve references. Explicit Aligned is retained. Invalid individual pick candidates are skipped; reference choices and geometry picking use the drawing's represented model rather than the active 3D component. |
| Orthographic views have unrelated scales and placement | Automatic Front/Top/Right views share a standard fit scale, with Top above Front and Right beside Front. Parent alignment and inherited scale remain editable; additional section/detail rows do not break the orthographic group. |

## Selection and presentation

Last-valid geometry remains available for navigation while a draft evaluates or
fails. New topology picks require the current valid evaluation, or the current
validated checkpoint input when editing a face/edge feature. Explicit upstream
reference repair and synchronized assembly placement previews retain their
existing exceptions. Cached geometry selections, hover guides, and candidate
menus are cleared when the selection source or occurrence placement changes.

The compact default composition consumes the same semantic edge records and
action identities as custom layouts. The complete edge metadata, editor values,
solved component poses, and mate reference frames remain available independently
of the native widgets. CSS still controls caption sizes, layout direction,
spacing, colors, and control sizes.

## Cache and compatibility

The backend is `monstertruck/0.4.1+fc2306852a46;adapter/35;solver/tcad-native-lm/16`.
This invalidates old derived geometry caches. No Monstertruck fork changes were
required for these fixes. Older sheet-metal references whose sidewalls previously
had indistinguishable whole-profile provenance may need explicit reattachment;
the application does not guess a replacement from position or display order.

Drawing contract 7 and drawing interaction contract 2 add perpendicular dimension
intent and an optional second exact curve reference. Existing point-based
dimensions stay as authored; Auto inference applies when creating a dimension.
Perpendicular dimensions require two exact projected parallel straight carriers.
Section intersection samples and unsupported curve carriers retain their stated
association limits.

The mate changes preserve free assembly motion rather than inventing a fixed
component in an existing document. Ground the intended base component explicitly
when editing a legacy assembly. Saved placement seeds are not overwritten by the
solver's display result.

## Validation checkpoint

Completed `cargo fmt --all --check`, `cargo check --workspace --locked`,
`cargo clippy --workspace --locked --lib --bins --no-deps -- -D warnings`,
and `scripts/package-macos.sh`. Packaging compiled the locked workspace,
validated Info.plist, verified nested executable and complete bundle signatures,
and replaced `dist/TCAD.app`. The repository remains private. The sample project
from the usability review was not modified.
