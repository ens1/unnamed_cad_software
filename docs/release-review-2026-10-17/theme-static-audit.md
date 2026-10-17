# Theme and presentation audit — 2026-10-17

Source reviewed: `c6b7343` (`Record Publish build and macOS package verification`).

This began as a read-only source review supporting the release dogfood session. The original findings below preserve the state of the reviewed commit. The session is now closed; authoritative runtime outcomes and remaining release issues are in [report.md](report.md). The final reviewed package is `4e95e72`. This supporting audit did not operate the UI or change personal preferences or CAD data.

The bounded menu, first-save suggestion and Nocturne → Classic Settings checks passed after correction. T4, narrow-width T5 and T7 remain unrun source concerns. The Settings height correction does not establish narrow-window or enlarged-scale acceptance. Recording was deferred at the user's request; [recording-index.md](recording-index.md) records the limited capture evidence.

## Findings

| ID | Priority | Evidence and impact | Recommended action / UI verification |
| --- | --- | --- | --- |
| T1 | P1 | **Classic's two source files have diverged.** `cad-core/src/presentation.rs:1657` loads `assets/ui/theme.json` as the bundled theme. Inspector heading/divider rules and `--section-*` variables exist only in `assets/ui/theme.css:749–798`; JSON has only a rule hiding confirmation-group headings. CSS also contains newer 3D sketch, weld-guide, material/finish-list, fabrication-guide and sketch-issue styling absent from JSON. Thus selecting Classic or F9 cannot supply the newer CSS grouping styles. | Use one authoritative bundled stylesheet or keep the representations synchronized. Compare a fresh Classic reset with an explicit CSS import, especially the left inspector's headings and dividers. Do not judge the shipping default only from an older personal snapshot. |
| T2 | P2 | **The editable Classic CSS contains an undefined variable.** `assets/ui/theme.css:745` sets selected sketch issues to `var(--accent)`; the root declares `--active`, not `--accent`. The style resolver rejects unresolved variables in checked resolution (`presentation/style.rs:569,1094`) and otherwise drops that declaration. | Change the reference to the intended declared variable. Verify selected sketch diagnostics after loading/exporting/reloading the CSS. Contextual validation may only expose the problem when the matching role is present, so a successful empty-workspace import alone is insufficient evidence. |
| T3 | P2 | **Several small state labels have weak contrast in Classic.** Selected tabs/tree labels use orange `#FF5A00` on pale orange `#FFE3D2` (approximately 2.56:1). Success green `#2DAA45` on cream `#F4F1E9` is approximately 2.68:1. The base size is 11 px, with many hints 10 px. Ordinary ink on cream is approximately 15.58:1. Ratios are static color calculations, not an accessibility certification. | Keep orange for borders, markers and fills; use dark ink or a darker orange for small selected labels. Inspect selected feature names, active tabs, validation status and disabled controls at normal and increased scale. Keep text/status meaning in addition to color. |
| T4 | P2, UI verification needed | **Top-level navigation has no overflow handling.** `cad_ui.rs:1150` puts the File/Edit/etc. row into a plain horizontal layout. `cad_ui.rs:1346` does the same for six command tabs plus Size. Only the second command row has horizontal scrolling (`1406`). The default context and command regions have fixed heights (`assets/ui/layout.json`). | Resize the app and try 150% scale with a long document name. Confirm all tabs, Publish, search and menus stay reachable. Consider an overflow menu or scrollable tab strip rather than letting essential actions move out of view. |
| T5 | P2, UI verification needed | **Settings permits a width smaller than its fixed two-column contents.** `settings_ui.rs:1050–1098` permits a 240 px-wide dialog while retaining a category sidebar of at least 90 px and a separate content column. Scroll areas are vertical only. Long category labels, theme names and Git/library actions may run out of horizontal room. | Resize Settings narrow and use 150% scale. Confirm full labels, editable fields and OK/Cancel/Apply remain usable. Introduce a content-based minimum width or a compact single-column category picker if this fails. |
| T6 | P2, UI verification needed | **Many controls have no distinct keyboard-focus styling.** The native button adapter publishes `focus focused` states (`presentation_controls.rs:424`), but Classic toolbar controls and tree rows force a fixed background and `border: none`; only sketch-reference/relation actions have an explicit focus rule. Those style overrides can suppress the native focus stroke. | Navigate with Tab/Shift+Tab and activate with Enter/Space. Check that the focus location is apparent before each action, including tree rows, toolbar commands and Publish. Add a shared focus treatment rather than only styling one command family. |
| T7 | P2, source-path concern | **Saved-theme fallback covers loading errors but not every validation error.** `presentation_resources.rs:714–734` reads the saved snapshot, then prefers a repository candidate when it parses. Full layout/style/context validation happens later in `reload` (`885–925`). If the live candidate parses but fails that later validation, the saved candidate is no longer available to that error branch. A running app retains its current theme, but a cold start can retain Classic rather than the promised saved theme snapshot. | Verify a repository theme with a parseable but invalid style/layout change across restart. On load, validate the live candidate before discarding the saved fallback. The current behavior still retains a usable presentation; this is a recovery-consistency issue, not evidence of CAD data loss. |

## Follow-up implementation and closeout

- **T1:** Classic now loads the authoritative `assets/ui/theme.css` through the existing portable CSS parser, cached after first parse. The outdated bundled JSON duplicate was removed. Its task-header/status, task-section, diagnostics and confirmation rules were first retained in CSS. Runtime JSON import/export is unchanged.
- **T2:** The selected sketch-issue style now uses a declared `--active-text` variable.
- **T3:** Orange fills and geometry markers retain `--active`. Small selected labels now use darker `--active-text`; selected orange material/finish and 3D sketch buttons use dark ink. Default success, reference and warning text colors are darker.
- **T6:** A shared focus border now covers ordinary, selected and pressed focus states. It uses the editable `--focus` variable, with no forced priority or change to hit geometry. Custom themes retain normal cascade control.
- Personal theme snapshots, repository theme files and project presentation packages were not changed. Existing saved themes will retain their own authored styling; choose Classic to review the new defaults.
- **T4, narrow-width T5 and T7 remain open and unrun.** Their recommendations are not claimed fixed by the theme asset work. Separately, Settings height correction `f1b3fcf` passed the exact Nocturne → Classic switch: title and confirmation actions stayed visible, and OK saved the preferences (RR-23). Smaller widths, other densities and invalid-repository fallback remain unexercised.

Static validation for the theme-asset follow-up: `cargo fmt --all --check`, `cargo clippy -p cad-core --lib --locked -- -D warnings`, and `git diff --check` passed. No automated tests or UI checks were performed by this supporting agent. The main reviewer rebuilt the application and recorded bounded runtime results in the report; dedicated contrast, keyboard-focus and selected-sketch-diagnostic checks were not completed.

### Wide native menu: source cause supporting the parent observation

The main reviewer observed an approximately 600 logical-pixel Edit popup for four short commands. The baseline source explains that width: `presentation_controls.rs:952` constructed `egui::Popup::menu` without a popup width. The bundled egui 0.36.2 default `spacing.default_area_size.x` is 600. Classic's `[role=menu-action] { width: 100%; }` then requested all of that available width during the first sizing pass, preserving the oversized area. Submenus used the same native helper.

Recommended adapter fix: provide a semantic, themeable menu surface width and viewport bound, or content-sized rows with justified hit areas. Do not shrink selection-action rows merely because they currently share a CSS rule with menu actions. No menu adapter change was made in this theme-only follow-up.

The menu adapter was subsequently corrected, and Classic menu-action labels now explicitly align left. The main reviewer observed an approximately 94 logical-pixel rebuilt Edit popup for the same four commands (RR-01). Other menus and keyboard-focus paths remain broader coverage gaps.

### Initial Save destination — corrected suggestion verified

The parent observed an unsaved UI-authored model propose `/Roller Fixture - Bearing Pedestal.tcad/project.json`. `document_identity_ui::begin_save_as` used the process working directory, which can be the filesystem root when the packaged app launches from Finder or another desktop launcher.

New unsaved projects now propose `~/TCAD/Projects/<name>.tcad/project.json`. Existing projects still propose a sibling copy, and previously entered destinations are preserved. Native Browse starts near the entered destination, current project, or user project directory; when a proposed folder does not exist yet, it opens the nearest existing parent without creating empty directories. Saving the chosen project remains responsible for creating its directories. Existing project files and explicit save paths are not relocated.

Validation: `cargo fmt --all --check`, `cargo clippy -p cad-desktop --bin tcad-desktop --locked -- -D warnings`, and `git diff --check` passed. No automated tests or UI actions were run for this supporting change.

The main reviewer later confirmed that a rebuilt empty Part1 proposed `~/TCAD/Projects/Part1.tcad/project.json`. Cancel created no file (RR-04). Other Save As and rejected-path variants were not completed.

## Existing paths that support the release workflow

These are source-supported paths, not additional runtime passes beyond the report.

- **Settings → Application → Themes** previews a theme immediately. Apply saves the personal default; Cancel restores the captured theme/layout. Theme selection is separate from engineering definitions.
- **Classic · Orange** is always listed. Optional themes come from installed repositories or imported portable packages; no sample part is needed.
- **Load theme…**, **Export theme…**, and **Edit layout and style…** expose import, reuse and source customization. Project packages under `ui/` take precedence over personal defaults.
- **Download themes pack** and **View theme pack…** lead to repository discovery. Content Library actions are explicitly immediate and are not undone by Settings Cancel.
- Personal theme preferences preserve a self-contained snapshot. Missing repository paths have a clear fallback notice with reconnect/download/reset actions.
- **F9** is processed before ordinary modal/publication keyboard handling. It recovers Classic even when a theme hides Settings. **`--reset-ui`** is the startup recovery path.
- Controls have stable roles, command bindings and labels. Truncated native buttons retain their full semantic/accessibility label and expose the full label on hover.
- Inspector headings have real collapse state and keyboard activation. Their visual treatment is separately styleable; the default-resource mismatch in T1 was corrected by loading the authoritative CSS.

## Deferred theme coverage

This matrix is retained for future review. It does not mark whole scenarios passed: the exact observations are in the report, and the remaining cases were not run during this closed session.

| Scenario | Evidence required in a future check |
| --- | --- |
| Empty Classic workspace | Empty Part1, clear origin planes, small usable commands, no sample-model names; visible route to first sketch. |
| Sketch and feature editing | Clear task title; obvious selected sketch members; distinguishable buttons, section headings and fields; required picks and validation status; stable Apply/Cancel placement. |
| Long feature tree and detailed property manager | Scrolling inside the intended region; visible selected row; readable names on hover; Apply/Cancel available without searching through long secondary sections. |
| Theme preview, Cancel, Apply | Change Classic to an installed alternate, then cancel; switch again and apply. Model, camera, selection and unfinished draft remain intact. |
| Repository theme missing / invalid | Last usable presentation retained, useful notice, recoverable picker/reset, no false claim that geometry failed. |
| Scale and window-size sweep | Normal size, smaller window, 125–150% scale. Inspect tabs, menus, Settings, field choices, context menu, Publish and drawing controls for clipping. |
| Keyboard-only controls | Visible Tab focus, readable disabled reasons, Enter/Space activation, Escape behavior, Cmd+Enter confirmation. |
| Model selection and previews | Selection, pending geometry, removed volume, errors and construction geometry remain distinct in both light and dark themes. Finish/texture remains readable under highlights. |
| Different documents | Saved project theme override is clear; personal default applies to a fresh project; switching documents does not silently lose the chosen project-specific appearance. |
| Recovery demonstration | F9 recovery from a deliberately inaccessible user composition, then restore the chosen theme without changing model geometry. |

## Deferred documentation ideas

1. **Choose a theme:** theme preview, Cancel, Apply and personal-vs-project scope.
2. **Adjust workspace density:** interface scale, resizing the tree/property manager, folding sections and finding commands.
3. **Create a personal theme:** export a package, explain layout versus style, reload a valid edit, recover with F9.
4. **Use theme packs:** download/clone, select an installed theme, explain the saved fallback when its repository is unavailable.

These are future clip ideas, not delivered tutorials or active recording instructions. The user stopped and deferred recording during this session. The recorder check and failed task capture are described in the recording index.

## T4/T5/T7 implementation after the review

- **T4:** The native top menu and command-tab rows now have horizontal overflow
  scrolling. Document titles use a bounded width, retain their full hover label,
  and no longer push commands arbitrarily far away. On wide workspaces the menu
  actions remain at the far end; a smaller workspace can scroll to every native
  menu, tab, Search, Publish and Size action. User-defined compositions retain
  their own placement and the existing semantic command identities.
- **T5:** Settings replaces the category sidebar with a compact category picker
  when the sidebar would leave less than 320 logical pixels for preferences.
  Category actions and staged preference behavior are unchanged. The preference
  body supports both-axis scrolling with bounded normal text wrapping; footer
  actions wrap instead of overflowing a narrow dialog. Existing themed dialog
  dimensions remain supported.
- **T7:** A live repository candidate now passes the same complete layout,
  renderer-property and contextual style validation before it can replace the
  self-contained personal snapshot. A parseable but invalid candidate falls back
  to that saved snapshot and provides a notice. Both source sets remain watched,
  so repairing the repository can restore its live theme on reload. Invalid
  saved snapshots still retain the currently usable presentation.

These are implementation changes, not runtime acceptance results. This follow-up
did not run code tests, assertion probes or UI checks. The resize/scale and invalid
repository scenarios in the deferred coverage matrix still need interactive
acceptance on the rebuilt application.
