# Structural members and profile libraries

The primary **Structural Member** workflow uses a layout sketch, selected sketch
entities, a pre-made section and a locating point. The separate **Custom Section
Along Path** command preserves TCAD's existing workflow for an authored section
sketch swept along a sketch path.

## Creating and editing a member group

1. Create a layout sketch in the part's model viewport. Lines, circular arcs and
   circles are supported member paths; construction entities may also be paths.
2. Choose **Weldments → Structural Member**, or find **Structural Member** in
   command search. The command can also start while editing a sketch: it retains
   selected sketch-member identities, requests the sketch's ordinary validated
   confirmation, then opens the structural-member editor.
3. Pick sketch entities directly in the model viewport, toggle their named path
   rows, or choose **Select all**. **Clear selection** keeps an explicitly empty
   group; it never silently selects the complete sketch. At least one valid path
   entity is required before confirmation.
4. Choose **Standard / collection**, **Profile type** and **Size**. A new install
   has no profile catalog. Use **Get libraries…** to clone the optional
   [TCAD weldment profile repository](https://github.com/ens1/tcad-weldment-profiles),
   register your own checkout, or import a catalog. The starter repository
   supplies nominal metric and imperial collections. Each group receives its
   own ordinary local section sketch and profile feature.
5. Under **Locate profile**, choose a named locating point in the list or click
   its marker in the profile diagram. The diagram's crosshair represents the
   authored path. Adjust **Rotation**, **Horizontal offset** and **Vertical
   offset** as quantities or parameter expressions.
6. Optionally set the existing weldment output, corner treatment, joint gap and
   start/finish shortening or extension. Confirm the current validated result.

Selected path entities have a dedicated viewport overlay. Section/profile/path
changes use the same document draft, evaluation worker and confirmation rules as
other features. Apply commits the section and member feature together as one
undoable operation; Cancel restores their starting checkpoint. A failed preview
does not authorize committing a previously valid proposal.

The underlying geometry remains native sketch-based member construction. A
group produces separate member solids rather than merging all structural stock
into one Boolean body. Further groups can use different section sizes and add to
an existing weldment. Trim/Extend, End Cap, sketch gussets and weld beads retain
their existing commands.

## Locating the section

Locating points include envelope center, side centers, corners and, where
appropriate, inner corners, circle quadrants, actual contour vertices and face
midpoints. Locators are fixed construction controls; they do not add material
contours or stray open chains to the section. A locating point may lie in a tube's
void or outside a concave section's material.

The selected locating point aligns with the authored layout path. Offsets are
signed X/Y translations in the **rolled section frame**, not world coordinates.
For section coordinates `p`, locator `a`, rotation `R` and offset `d`, the
cross-section placement is `R × (p − a + d)`. The diagram is a disposable view of
that placement; its polygon/circle drawing never becomes geometric authority.

Offsets move stock relative to its authored layout centerline. Manufacturing
centerline length continues to describe that layout path's evaluated developed
length, after the existing end treatments. It is not a new measurement of an
offset stock edge.

## Optional starter sections and precision

[`nominal-sections.json`](https://github.com/ens1/tcad-weldment-profiles/blob/main/nominal-sections.json)
in the separate private starter repository contains 104 original profiles in nine families: square tube, rectangular tube,
round tube, angle, channel, I beam, flat bar, solid round and solid square.

All coordinates and radii use authoritative `f64` millimetres. Imperial labels
use 25.4 mm per inch; the collection's `units` field describes its nominal labels
and does not change coordinate units. Instantiation creates native line/circle
sketch members with explicit `fix_x` and `fix_y` millimetre constraints.

These profiles are **nominal design envelopes**. Straight-sided sections have
sharp corners. They omit rolled root/corner radii, flange taper, manufacturing
tolerances and supplier certification. Neither collection claims certified ISO,
EN, AISC or ASTM dimensions. Engineering material and visual finish remain
independent part properties. Catalog size text is procurement/source information;
measured section dimensions still come from evaluated geometry.

Member paths currently use planar straight lines, analytic circular arcs and
closed circles. Section construction requires one closed material region with
optional holes. Group size is bounded to 1–1,024 selected path members. Existing
joint limits remain: two-member miter joints, independent butt ends and
prioritized butt trimming against a convex straight-sided primary section.
Circular joints require tangent-normal end caps. Arbitrary branch coping,
nonplanar paths and complex multi-member intersections are not established by
the presence of a profile catalog; unsupported cases report diagnostics.

## Importing and exporting libraries

**Content Libraries** manages Git checkouts, clones, forks, local creation,
updates and publication. Profile entries in `tcad-library.json` point to catalog
JSON using kind `weldment_profiles`. The guided editor reads enabled repository
entries and names their source. Fork a repository to author different stock;
refresh after changing its catalog data. Repository updates never replace a
committed section's saved profile.

The guided editor also retains **Import profile library…** and **Export profile
library…** for portable JSON interchange. Existing personal catalogs remain
available. Imports validate the whole catalog before atomically updating its
personal copy; imported catalogs cannot replace the snapshot stored in a model.
Export writes the selected profile's complete source catalog when available. A
member continues to evaluate from its saved profile if its source library is
removed; exporting that entire library requires restoring the source catalog.
The personal adapter remains bounded to 32 loaded libraries and bounded local
JSON reads.

The portable v1 grammar is implemented in
[`weldment_profiles.rs`](../crates/cad-core/src/weldment_profiles.rs).
The [starter repository README](https://github.com/ens1/tcad-weldment-profiles#readme)
includes the editable source and library guidance. A catalog contains:

- `version`, `id`, `revision`, `name`, `collections` and `profiles`.
- Each collection's `id`, `label`, nominal `units` and `notes`.
- Each profile's `id`, `revision`, `collection`, `family`, `label`, `size`,
  `notes`, `contours` and named `locators`.
- Polygon contours with ordered `points`, or analytic circles with `center`
  and `radius_mm`. The first contour is the exterior; later contours are holes.
- Locators with stable `id`, human-readable `name` and millimetre `point`.

`family: "custom"` permits another supported polygon/circle section without
mislabeling it as a conventional stock family. Family-specific contour checks
reject inconsistent nominal tube/bar definitions. Validation also rejects
nonfinite values, collapsed/ambiguous corners, intersecting or touching
boundaries, holes outside the exterior, nested holes, missing collection
references and duplicate identities. Open paths, multiple material islands,
arcs and spline contours are outside this catalog version.

The core contract bounds JSON to 4 MiB, 2,000 profiles, 32,768 contour controls
and a shared 4,000,000 validation-pair budget. Individual profiles permit at most
17 contours, 1,024 controls, 256 vertices per polygon and 64 locators. Import
checks are bounded source sanity checks. The current native section evaluation
still validates topology and geometry before Apply.

## Saved source and replacement boundaries

`tcad.weldment` v3 retains the existing section/path references and adds an
explicit member-selection mode, rolled-section X/Y offsets and an optional
`library_profile`. Older member groups deserialize with the previous complete-
path semantics and zero profile offsets.

`tcad.weldment_profiles` v1 owns the catalog source contract. A committed group's
`Origin` stores the catalog ID/revision/name and complete profile entry. Its
deterministic SHA-256 digest identifies that saved source. Evaluation needs no
live catalog file, network service, renderer object or kernel object. Updating,
removing or moving a catalog does not change an existing member.

Library insertion and Make Unique can remap local sketch, point and member IDs.
The external catalog snapshot remains literal source data. Profile-source
matching compares exact authored geometry and fixed constraints modulo those
local identities, so a renamed or remapped sketch retains its association. Each
point must still have its exact fixed X/Y millimetre source constraints.

Editing a generated section's source requires **Use custom section**, or the
group reports that its pinned profile has changed. **Restore saved profile**
recreates the pinned section in the current draft; it does not consult a newer
catalog revision. Explicit profile selection is the route for changing stock
size. Lightweight evaluated/cut-list `ProfileLabel` metadata includes source
identity, revision, nominal label and digest without copying complete contour
geometry into every manufacturing row.

The editor's groups, fields, actions, profile diagram and path annotations expose
semantic descriptors under `tcad.presentation.structural-member` v1 and
`tcad.presentation.structural-profile-locator` v1. Their default UI consumes
those descriptors; user layout and CSS-like styling remain independent of the
saved engineering definitions.

This document records implemented source behavior and limits. Compilation and
linting do not establish interactive acceptance or geometric coverage; no test,
probe or UI acceptance run was performed for this implementation.

Implementation checks on 2026-10-12: `cargo check --workspace --locked`,
`cargo fmt --all --check`, `cargo clippy --workspace --lib --bins --locked -- -D warnings`
and `git diff --check` passed. No code tests, probes or interactive checks were run.
