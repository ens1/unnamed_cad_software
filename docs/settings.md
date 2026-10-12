# Settings

TCAD separates preferences saved with the current model from personal application
preferences. Opening a document retains its saved model settings; application
defaults apply to models created afterward.

Open **Settings…** from **Edit** or **Tools**, or press **Cmd+,** on macOS
(**Ctrl+,** on other platforms). The compact, resizable dialog groups searchable
preferences under **Model** and **Application**. Model categories are **Units and
precision**, **Geometry and parts**, and **Lighting**. Application categories are
**Interface**, **Themes**, **Model display**, **Trackpad and navigation**, **Recovery**, and **New model defaults**.

Current-model settings include the displayed length unit, length and angle
decimal places, evaluation tolerance in millimetres, and the default visual finish
for new parts in that document. Lighting is also saved with the model. These
preferences belong to CAD source and use the same draft, evaluation and undo
contract as modeling operations. Changing display units does not rescale authoritative geometry or replace explicit units
in saved expressions. An existing part keeps its assigned finish when the
default for new parts changes.

Application preferences include interface scale, trackpad navigation profiles,
gesture directions and sensitivities, automatic reload of external presentation
files, recovery snapshot enablement and interval, and defaults for new models.
The new-model pickers accept built-in, catalog or current-project materials and
finishes. Custom choices retain independent personal source snapshots. Finish
textures are copied into a checksum-pinned personal resource store; new documents
receive their own pinned files in the unsaved resource root before assignment.
The default snapshot does not keep a live link to the source project. Imported
texture bytes are bounded and fully decoded before the preference is accepted;
missing, altered or unsupported assets block Settings Apply and document creation.
Selecting a built-in clears that custom snapshot. Cancel leaves saved preferences
unchanged; unreferenced immutable import files may remain. Layout and CSS-like
theme resources remain independent presentation files.

**Application → Themes** provides a picker for **Classic · Orange**,
**Nocturne · Violet**, and imported packages. Picking a theme previews its whole
layout and styling immediately. **Load theme…** accepts a presentation manifest,
CSS stylesheet, or JSON theme; a stylesheet keeps the current layout. Imported
themes are copied into the personal library with their local dependencies so
they remain available after their source files move. **Export theme…** writes a
portable package of the current layout and styling. **Edit layout/style…** applies
Settings before opening the presentation source editor.

**Apply** remembers the selected presentation as the personal default, with
project-specific presentation resources still taking precedence. **Cancel**
restores the previous layout, stylesheet, active composition, and reload sources;
imported library entries remain available. Theme changes preserve the document,
modeling draft, selection, camera, and undo history. **F9** recovers Classic even
inside a hidden or heavily customized Settings dialog; there it remains an
unapplied preview. [Nocturne](theme-example.md) demonstrates a right inspector,
bottom tool dock, circular and pill buttons, larger targets, and a dark palette.

**Application → Model display** controls part edges, circular centerlines/axes on hover, and straight-edge midpoint guides on hover. All three default to enabled, including older preference files that omit these additive fields. Changes preview immediately and follow the personal Apply/Cancel path. Edges follow the displayed part/history stage and are depth-tested over the shaded model. Hover guides disappear when the pointer leaves or a drawing/navigation gesture owns it. Native centers are optional for carriers the kernel can prove; no center is invented for an unsupported curve. Theme overrides may alter the displayed visibility, colors, widths and hover distances without changing saved preferences.

Interface scale, model display, navigation and external reload changes preview immediately.
**Apply** saves personal preferences and commits model changes together only
after the current model evaluation validates; pending or failed geometry cannot
commit. Model settings changes create one undoable operation. **OK** applies and
closes; **Cmd+Enter** (**Ctrl+Enter**) does the same. **Cancel**, the close button or
**Esc** discards changes made since opening or the most recent successful Apply.
Personal preferences are persisted immediately by Apply; model preferences reach
the project files when the model is saved. Opening Settings during an active
modeling operation permits personal preference changes while model preferences
remain read-only, preserving that operation's draft.

The dialog and fields publish semantic roles and stable identities, including
`settings:dialog`, `settings:category:<category>` and
`settings:field:<scope>:<field>`. Default controls read the same descriptors and
setters available to user-defined compositions. Theme selectors for
`settings-dialog`, `settings-category`, `settings-field`, `settings-field-label`
and `settings-action` control the presentation; dialog `width` and `height` and
category-panel `width` are also user-styleable. Model values and personal
preferences contain no colors, positioning or widget implementation details.
The theme picker uses `settings:field:presentation:theme`; its choices and
`settings.theme-load` / `settings.theme-export` actions share the same contract
with custom compositions.

The desktop application preference adapter writes version 1 JSON to
`settings.json` beside the existing personal `recovery/` directory. On macOS this
is `~/Library/Application Support/TCAD/settings.json`; on Windows it is
`%LOCALAPPDATA%/TCAD/settings.json`; on Linux it is
`~/.local/share/tcad/settings.json`. Writes validate the entire preference set
before an atomic replacement. Interface scale is bounded to 75–150%, recovery
interval to 1–3600 seconds, and gesture sensitivities use the navigation
contract's existing finite ranges.

When the unified file is absent, valid existing `navigation.json` preferences are
imported into it. After migration the unified file is authoritative; the legacy
file remains untouched. A malformed, oversized or newer settings file reports a
diagnostic and loads factory defaults without overwriting the original.
Preference reads are bounded to 256 KiB, with a 32 KiB legacy navigation limit.
`--reset-ui` bypasses persisted application preferences without replacing them.

The application preference contract and current-model contract have independent
version identities. Neither stores Bevy, egui or geometry-kernel objects. New
fields can acquire defaults while older saved preferences remain readable.

Recovery reloads the current draft, or unsaved committed definitions when no draft
is present, after reopening the matching project against the recorded saved base.
Recovery files are not substitutes for saving a project. The CLI also uses the
model’s saved evaluation tolerance unless an explicit `--tolerance` overrides it.

To replace the native dialog with a custom composition, hide
`[role=settings-dialog]` with `display: none` and bind the published category,
field and action IDs. The settings transaction and Escape/Cancel recovery remain
available.
