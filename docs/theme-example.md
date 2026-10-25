# Optional presentation examples

The [theme repository](https://github.com/ens1/tcad-themes) includes three complete
presentations. Install or update it through **Content Libraries**, then select a
theme under **Settings → Application → Themes**. Classic Orange remains the only
bundled theme. Each optional theme is editable CSS and layout JSON; none changes
modeling behavior or installs sample geometry.

- **Fusion 360-inspired** (`fusion-360/`) uses a pale canvas, blue selection, top
  tools, a persistent left browser, a right operation inspector, and a bottom
  history/navigation strip.
- **SolidWorks-inspired** (`solidworks/`) uses a compact gray command bar, a left
  tree/property manager, square controls, warm selected tabs, and a light blue
  canvas.
- **Nocturne** (`nocturne/`) demonstrates a larger departure from the default,
  described below.

The two CAD-inspired themes share `cad-controls.css` within the repository.
Each package retains its own layout, palette, and control sizing. Loading or
exporting a package resolves its local stylesheet dependencies.

## Nocturne

Nocturne is an optional presentation package in the starter theme repository, under `nocturne/`. Clone that repository through **Content Libraries**, then choose Nocturne in **Settings → Themes**. Only Classic Orange ships in the application.
It uses the same document, commands, selection, draft validation, and undo history
as the default CAD workspace. It contains no sample part or CAD definitions.

The example demonstrates changes beyond a palette:

- The model tree and property manager move to a resizable right inspector.
- Feature and sketch tools move into a full-width bottom command dock.
- A narrow, scrollable left rail presents circular Settings, command search,
  fit, and zoom actions, plus square Undo and Redo actions.
- Commands use larger pill buttons, larger icons, and more generous targets.
- Apply becomes a lime circle; Cancel becomes a purple pill.
- Midnight purple surfaces, cyan selection, lime validation, and monospace
  typography replace the light orange workspace.
- The orientation cube moves to the lower-left of the model viewport.
- Settings receives a larger rounded surface and different category, search,
  field, and confirmation styling.

`presentation.json` names the package and selects `nocturne-workspace`.
Its `inherit_defaults: false` flag makes the composition and stylesheet
self-contained, so the default theme's more specific selectors cannot leak
into the example when it is loaded or exported.
`layout.json` controls composition and semantic bindings. `theme.css` controls
appearance and viewport presentation. The package is self-contained; these
files can be copied, edited, and loaded without recompiling TCAD. The CSS
variables at the top are a convenient starting point for variations.

The layout also includes themed `instrument-rail` and `graph-workspace`
compositions because the native Features and Graph tabs use those layout
identities. The Graph composition retains one model viewport and adds the
dependency graph beneath it, with editing controls on the right.

The command dock consumes the existing command-manager adapter, whose tool
groups follow the registered modeling tools. Zoom actions demonstrate repeated
semantic bindings. Presentation resources never own another copy of a CAD
parameter or authorize Apply before the current evaluation is valid.

The portable style grammar supports solid colors, borders, rounded or circular
button shapes, built-in proportional or monospace fonts, spacing, sizing,
layout flow, anchors, and viewport colors and lighting overrides. Browser-only
features such as gradients, `calc()`, web fonts, and box shadows are not part
of the current adapter. Layout and stylesheet import diagnostics retain the
last usable presentation when a resource is invalid.

Nocturne is a deliberate, more spacious alternative. Its dimensions and target
sizes are ordinary editable resources. Geometry, materials, and physical
dimensions retain their authoritative model definitions when it is selected.

Installed repository themes load directly from the checkout. Layout and CSS edits can reload through the normal presentation watcher, including after Settings Apply and app restart. The personal preference also keeps a self-contained last-applied snapshot, so removing the optional repository does not leave an unusable workspace. Imports from a repository theme must stay inside that repository; invalid files retain the last usable presentation. To create a variant, fork the content repository in Content Libraries, edit its files, refresh, and select the fork's theme.
