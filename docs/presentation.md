# Semantic presentation, layouts and themes

The workspace is a composition of semantic components. Geometry and application transactions do not depend on the layout. The bundled resources are `assets/ui/layout.json` and `assets/ui/theme.json`, with the equivalent editable CSS at `assets/ui/theme.css`. Project packages are saved under `ui/`; personal defaults live in TCAD’s application-support presentation directory. The default layout has conventional CAD command tabs above a left feature tree/property manager and a shared model viewport. The reference images guide the spacious viewport, quiet surfaces, restrained selection cues, and in-place editing; the sample names and geometry are examples, not startup content.

A component has `role`, optional `bind`, optional `command`, and optional `widget` and `label`. Nodes can override these bindings. A field or command binding takes precedence over a native composite role, so replacing an editor does not require privileged widget names. Recognized workspace roles are `document-context`, `command-manager`, `status-bar`, `view-navigation`, `dependency-view`, `property-manager`, `preview-status`, `apply-action`, `cancel-action`, and `model-viewport`. `command-manager` provides Features, Sketch, Assembly, Evaluate, and Graph tabs. Its semantic tool catalog groups modeling under Create, Remove, Modify, Pattern and Reference, and sketching under Draw, Shapes, Curves, Reference and Constraints. `dependency-view` provides the feature tree, numbered History stations, or dependency cards. The graph workspace places cards below the shared viewport; its Assembly scope shows components, mates, and the assembly result. Cards have explicit ports, draggable placement, pan/zoom and contextual edit actions. `property-manager` hosts the active tool editor beside the model; `parameter-instrument` remains a compatible alias. Tool groups can use `assembly-editor`, `materials-editor`, `drawings-editor`, `manufacturing-editor`, `versions-editor`, `review-editor`, `bom-view`, `presentation-editor`, `sketch-editor`, or `library-editor`. A sketch editor provides controls; its geometry always appears in the model viewport. Generic components can bind a semantic entity ID, bind `definition-uuid::parameter-id`, or invoke a registered command. Parameter duplicates edit the same domain parameter and draft.

A layout node can have `id`, `role`, `use`, `layout` (`row` or `column`), `width_logical_px`, `height_logical_px`, `grow`, absolute `position: [x,y]`, `hidden`, `resizable`, `classes`, and `children`. Give duplicated components explicit IDs to retain presentation identity through reordering. Row/column allocation now includes gaps, CSS-order padding insets, min/max sizes, cross-axis alignment, justification, parent-relative absolute positioning and nine anchors. CSS sizes also accept percentages against the parent. `overflow: auto` or `scroll` creates an actual scrolling container; clipping follows the declared hierarchy. Grid, wrapping, media queries and a browser DOM are outside this native subset. Explicit node identities survive reordering; anonymous placement identities use their structural path.

The optional `editing` object overrides `width_logical_px`, `height_logical_px`, and `grow` while the property manager is active. The bundled rail uses it to exchange the tree and tool controls within the same dock. The Model/History/Graph selectors let the user change the rail view; Edit returns to the active controls. While editing, a compact history remains above the scrollable fields. These overrides belong to the layout resource and can be changed or removed by the user.

Modeling editors use the common validated preview transaction. The compact orange Apply action waits for the matching evaluation and commits one undoable change. The sketch editor remains open while validation is pending or fails, retaining fields for correction. Further edits cancel the queued confirmation and update the preview. Cancel restores the session checkpoint taken when the operation started, retaining any edits that predated the operation. Existing Apply/Cancel command bindings remain available for custom layouts and general document edits.

History stations now invoke the view-only `history.inspect` command. A matching valid evaluation supplies cached intermediate body meshes and sketch visibility at each feature. Clicking a station replaces the displayed active part with that stage; other parts retain their latest geometry and assembly placements/configurations remain intact. Later rows are dimmed. Mesh rendering, picking, selection outlines and Fit use the same cached stage, so invisible later faces and sketches cannot be selected. The cursor does not change definitions, outputs, suppression, draft or undo state, and switching stations never submits an evaluation request. **Latest model** in the rail, **Latest** in the viewport, the `history.latest` command, or Escape from the viewport restores the current display. Feature editing leaves inspection before opening its normal editor; geometry modeling actions explain that Latest must be restored first.

History is feature-sequence inspection using current parameter values, separate from Git revision comparison. A pending/failed evaluation, missing/incompatible cache or non-causal feature order reports an explicit reason instead of rebuilding on click or substituting final geometry. Normal evaluation prepares the caches once. Inspection cannot start during an active modeling operation. The cursor is transient and saved projects reopen at Latest. Semantic `history-station` IDs are `history:definition-id::feature-id`; they expose the versioned target, index, availability and disabled reason. Bind `history-view` for the inspection label and Return to Latest action in a custom layout.

The Size menu controls UI scale (0.75–1.5) and toolbar height (60–130 logical pixels), with Compact and Comfortable presets. Toolbars scroll horizontally; tree, history and controls scroll vertically. These controls edit presentation state/layout without changing the source model. `ui/workspace.json` stores UI scale, toolbar setting, graph scope, graph positions, pan/zoom, sketch annotation positions and dimension/relation visibility. Save presentation writes the current resized layout and state. Arbitrary saved layout heights are respected until the user explicitly changes sizing.

The viewport has a clickable view cube and leaders for the selected feature/value or geometry measurement. Pinned assembly component references have separate labels; edit-in-place offers Ghost others, Hide others and Finish Editing in the viewport. These overlays remain native composite widgets. New documents stay empty and use the user's own part names.

Click an edge, face or sketch geometry in the model viewport to open a compact action popup beside the selection. Primary clicks in the tree, History, graph, property manager and semantic selection controls select or inspect without opening a popup. Clicking empty viewport space clears the selection without opening actions. Feature selection and popup opening are separate presentation operations; double-click editing and explicit right-click menus remain available. Edge actions include Fillet and Chamfer. Faces offer Sketch on Face, Shell with that face removed, and Draft; Sketch on Face explains when the selected face is not planar. Sketches offer boss/base and cut operations; solid features offer body modifiers and patterns. Edit, Suppress/Unsuppress, and Delete target the selected feature. Geometry actions prefill the existing modeling editors and keep the common validated Apply/Cancel transaction. While an operation is active, geometry clicks continue selecting its inputs and expose applicable view commands; competing modeling operations have disabled reasons. Active sketches project their own member/point/reference/constraint actions. Clicking outside, navigating, starting an action, or pressing Escape dismisses the popup while retaining the selection. Double-clicking a sketch or feature opens its editor.

Sketch members and endpoint/center handles in the model viewport support the same primary-click action popup. Member, point, reference and constraint controls in the property manager only select their target; double-click a dimension row to edit its value. Shift/Command-click adds or removes an entity; Origin, X Axis and Y Axis selectors make fixed sketch references accessible even when geometry overlaps them. Relations shown in the popup or property manager are filtered by entity types. On X/Y Axis aligns an entire straight line, or places a curve start/center or point on the axis. Coincident with Origin anchors a member's start/center; selecting an endpoint explicitly determines which point is anchored. References are the active sketch's plane-local frame, including for face-attached sketches.

Smart Dimension is available in the Sketch command tab, selection popup, command search and **D** shortcut. Select a member or Shift-select two points/lines, then invoke it; an empty selection starts a pick sequence. The property manager offers applicable dimension types and a live expression field. For a point pick sequence, click the second entity or press D again for coordinates from the origin. Choose **Create Named Parameter** to bind the dimension to a new D1/D2/etc. parameter, then edit it under Named Parameters or another parameter binding. Sketch and downstream geometry previews update before Apply; invalid/conflicting constraints retain the last valid preview and block confirmation. Cancel restores the operation checkpoint, including newly created dimension parameters.

Click a dimension/relation label to edit or remove it through the popup; double-click a dimension to open its value editor. Drag labels to change presentation-only positions, or hide Dimensions/Relations in the property manager. Style annotations with `[role=sketch-annotation]`, `.dimension`, `.relation`, or `#constraint-id`; supported properties include color, background, font-size, font-family, padding, border-radius, display, and initial left/top offsets relative to the geometry anchor. Saved dragged offsets override initial offsets. `--sketch` sets the default sketch geometry color. Semantic `sketch-member`, `sketch-point`, `sketch-constraint`, and `sketch-reference` bindings expose the same selection and editing commands independently of annotations. Native controls remain docked.

In Select / Move, sketch handles drag endpoints, arc through-points, centers and spline controls. Selected members expose diamond move handles; circles expose square radius handles, and ellipses expose square major/minor radius handles plus a rotation handle. A selected spline shows its control polygon. Center rectangles include a center point related to a construction diagonal. New rectangles infer horizontal/vertical relations but do not automatically fix position or size. Add Smart Dimensions or origin/axis relations to constrain those freedoms.

The cursor is a temporary solver goal: dragging moves related geometry while preserving engineering constraints. Fixed positions and dimension/parameter-driven size handles show locked styling and explain their driver. Alt bypasses grid snap. Each gesture adds one sketch history entry; Escape restores that gesture's starting document and preview without discarding prior pending edits. Leaving the viewport freezes movement; releasing keeps the result pending, and loss of window focus cancels capture. Navigation is suspended during capture. Apply still requires the current valid worker evaluation, and Cancel restores the sketch operation checkpoint. Invalid drag candidates retain the valid sketch. The click menu's Drag Handles action preserves selection and returns to Select / Move.

Semantic `sketch-handle` elements carry version 1 handle targets, stable IDs, plane-local f64 positions, labels and disabled reasons. `sketch.select-handle` selects the owner and exposes the native drag controls; other renderers can submit goals through the core handle/solver contracts. Style `[role=sketch-handle]` with `.point`, `.center`, `.control`, `.move`, `.size`, or `.rotation`; states are ready, hovered, selected, locked and dragging. Supported properties are color, background, size, hit-size and display. Native sizes are clamped to 4–30 logical pixels with a larger hit target; these appearance values never change source geometry.

Part/component rows, named parameter labels, material labels, planes, Origin, and assembly graph objects also publish relevant contextual actions for explicit commands and docked action bindings; primary selection does not open a menu. Parameter edits use the same identities and shared draft during sketching; they do not switch away from the active sketch editor.

Selection capabilities are published by `cad-core::selection_actions` as `selection-action` semantic elements, with command identities, target references, groups, availability, and disabled reasons. Bind a custom component to `selection:feature.chamfer` or another published selection action to present the same command elsewhere. The descriptors appear in the binding catalog when an entity is selected. A `selection-actions` layout role displays the current selection's actions in a dock of your choosing. The default floating presentation has ID `selection-popup`; style its width, height (scroll limit), padding, gap, font size, colors, border, radius, and position through theme rules. `#selection-popup { display: none; }` disables the floating menu if you prefer a docked action list.

For example, add a dedicated parameter component and place it independently:

```json
{
  "components": {
    "bore-value": { "role": "parameter-field", "bind": "part-uuid::bore" }
  },
  "node": {
    "id": "bore-control",
    "use": "bore-value",
    "position": [900, 120],
    "width_logical_px": 240,
    "height_logical_px": 150,
    "classes": ["large-control"]
  }
}
```

The example is an excerpt: merge the component into `components` and the node into the chosen layout's `children`.

Theme JSON contains `schema_version`, `variables` and `rules`. A rule contains a `selector` and a `properties` map. CSS text can also be loaded. Selectors support `*`, `:root`, IDs, classes, comma lists, and `[role=...]`, `[layout=...]`, `[state=...]` attributes. Compound selectors, child (`>`) and descendant selectors use the actual placement ancestry. Attribute selectors also accept quoted IDs, such as `[id="command-toolbar/feature.extrude"]`. State tokens support `:hover`, `:focus`, `:selected` and `[state=disabled]`. Rules cascade by specificity, order and `!important`; inherited typography and scoped custom properties follow their ancestors. Variables resolve through `var(--name, fallback)` with cycle and unresolved-variable diagnostics. Supported primary-workspace properties include `color`, `background`, `padding`, `gap`, `font-size`, `font-family`, `border`, `border-radius`, `left`, `top`, `width`, and `height`. This is a validated CSS-like subset. Unknown portable properties and undeclared `renderer-` extensions are rejected. Supported property names are published by `SUPPORTED_STYLE_PROPERTIES`; applicability still depends on the target (for example, `icon-size` affects an icon control). Colors support short/full hex with alpha, transparent and the documented basic names. Custom CSS geometry never changes engineering materials.

```css
:root { --accent: #5B47EA; --ink: #181820; }
[role=instrument-rail] { background: #EDEBF5; }
[role=apply-action] { background: var(--accent); border: 2px solid var(--ink); border-radius: 50%; }
#bore-control { left: 800px; top: 130px; width: 260px; font-family: monospace; }
```

Use **Layout and theme → Apply source** to validate and apply edited sources. External file changes reload automatically after a debounce; turn **Reload external edits automatically** off to pause. Pending source edits prevent an external reload from overwriting editor text. Layout and theme are replaced together only after validation; failures keep the last usable pair and display a diagnostic. Reloading releases obsolete pointer captures while retaining the document, draft, undo history, selection and camera. **Save to project** writes a portable package under `ui/`; **Save as my default** saves application preferences separately from CAD files. Package import/export flattens dependencies into local JSON resources and publishes the manifest after both files are written. F9 restores default presentation and navigation preferences even when all normal controls are hidden, clearing view roll. `--reset-ui` bypasses persisted presentation/navigation overrides at startup. See [view navigation](navigation.md) for saved profile/inversion/sensitivity fields, gesture ownership and actual platform capabilities.

The Layout editor includes a searchable semantic binding catalog. `tcad describe` emits the same catalog as JSON. Every scalar document field has a `source:/...` semantic binding with typed JSON, an accessibility label, a current JSON pointer, and edit availability. Entity arrays use `@stable-id` in their binding identities so reordering does not change a feature or dimension binding. For example, `source:/parts/part-id/features/@feature-id/operation/distance` binds an extrusion expression. These fields enter the common draft through document commands; schema/dependency validation still applies. Boolean fields render checkboxes; string, expression, and numeric fields accept exact entry. The document and definition identity fields remain read-only in these widgets. Use the source editor for deliberate source migrations.

A command component invokes a catalog identity independently of its appearance. `command:<identity>` semantic descriptors include labels, availability and disabled reasons. Feature bindings also expose edit/suppress/delete actions targeted at that feature. Sketch-member descriptors expose geometry, construction state and a targeted edit action; assembly-mate descriptors expose their component relationships. Tool descriptors include semantic group labels independently of layout/theme.

Default native widgets use `--panel`, `--ink` and `--active`; `--reference` colors pinned callouts. Optional `--warning` and `--error` override native diagnostic text colors, including assembly inspection results. The panel color selects the native light/dark widget palette, independent of engineering materials. Viewport styles accept `background`, `selection-color`, `preview-color`, `comparison-color`, `ghost-opacity`, `inspection-first-color` and `inspection-second-color`. Hex colors may include an alpha byte, such as `#26D98561`. These are presentation colors; engineering material definitions remain authoritative.

Native CAD editors remain available as registered composite views. Their document fields can be replaced or duplicated through stable bindings. Toolbar actions, command tabs, individual menus and entries, feature/history rows, selection actions and confirmation controls now use the shared styled-control adapter. Smart Dimension value/reference fields, resource source/watch fields, sketch block source/placement, configured feature dimensions, and drawing symbol fields publish transient setters. Viewport context, start hint, view cube, selection candidates, isolation controls and navigation hint have stable styling identities and parent-relative sizing/anchors. Some specialized transient editors and geometry-attached overlay internals still use composite adapters; their full migration remains presentation work, rather than a limitation of the contracts. Native file pickers remain operating-system dialogs. Duplicate model viewports currently share one camera and render texture.


Sketch editing publishes stable commands `sketch.convert-entities`, `sketch.offset-entities`, `sketch.fillet`, `sketch.toggle-construction`, and `sketch.detach-conversion`. The default Sketch tab groups these under Edit Entities, and selection actions expose applicability/disabled reasons. Model edges and face boundaries expose Convert Entities while a sketch is active. `sketch-modifier` semantic elements expose saved source relationships, output member IDs, expression values and edit/delete/detach commands. Transient `sketch-edit` elements expose operation expression, direction, construction, source-link state, selected topology references, errors and `sketch.accept-edit` / `sketch.cancel-edit`. The default property manager renders live controls; the same command handlers serve custom layouts. Construction state is uniform for a selected member set and available for every member type. Driven controls carry disabled reasons instead of changing source relations through dragging.

Navigation preferences publish `view:navigation` and `view:navigation:<field>` bindings separately from document source fields. Boolean inversion/profile controls, sensitivity ranges, precision options and `navigation.*` action identities use the same saved application-owned contract in a custom layout or the docked Controls editor. Camera roll is presentation state and uses the common renderer/pick/pan/zoom frame; it is cleared by named/normal views. Platform gesture events remain a replaceable desktop adapter. See [navigation contract and limitations](navigation.md).

Sketch members, selected whole sketches, model edges and faces use the viewport `selection-color` consistently. Whole-sketch viewport selections retain the complete occurrence path; tree selections use their selected definition and active occurrence. A root part has no assembly occurrence path. Face feedback includes a depth-tested translucent native-face display overlay and its boundary outline, with body, producer, topology and full-path identity checked against the same evaluated geometry used by picking. During modifiers that pick their starting body, feedback follows the input checkpoint. The face fill uses `selection-opacity` in 0–1 (default 0.32), multiplied by the alpha of `selection-color`; source geometry and engineering material remain unchanged. Sketch member selections clear unrelated model selections while Smart Dimension retains its separate pending model-edge target. History navigation clears transient occurrence selection and uses its displayed cached station.


## Collections and future extensions

A semantic collection uses `repeat` and `template`. Filters include `role`, `id_prefix`, explicit `ids`, `relationships`, `owner`, `states` and a bounded `limit`. Placeholders `{id}`, `{label}`, `{role}`, `{owner}` and `{command}` resolve from the repeated descriptor. Templates inherit that semantic binding; children can override it. Repeated placement identities combine the collection and stable semantic identity, independently of the data setter.

```json
{
  "id": "my-tools",
  "role": "semantic-command-collection",
  "layout": "row",
  "height_logical_px": 40,
  "overflow": "auto",
  "repeat": { "role": "command", "id_prefix": "command:feature." },
  "template": { "role": "command-action", "widget": "action", "width_logical_px": 108 }
}
```

The bundled **semantic-workspace** composition uses command, feature and active-operation field collections next to the same model viewport. It demonstrates an editor assembled from bindings. New registered commands and matching fields appear without updating the layout. `widget` chooses auto, label, value, action or field presentation; values and actions remain independent of the widget.

The desktop catalog adds stable `command-toolbar/<command>`, `command-menu/<command>`, `command-tab/<tab>`, `navigation/<mode>` and `menu/<name>` bindings. `operation:<feature-id>/<field-path>` exposes active operation scalar fields; `source:/...` remains the complete document source catalog. Edits resolve the stable field identity against the current source before using its JSON pointer, preventing a reordered array or duplicate control from targeting stale data. Draft Apply/Cancel bindings use the same operation confirmation availability and checkpoint as native controls.

```css
:root { --control-height: 22px; --target-size: 26px; }
.toolbar-control { font-size: 10px; icon-size: 16px; height: var(--control-height); }
.toolbar-control:hover { background: #E4E0D8; }
[id="command-toolbar/feature.extrude"] { color: #D64C00; }
[id="viewport.view-cube"] { right: 8px; top: 72px; width: 80px; height: 84px; }
[id="viewport.navigation-hint"] { display: none; }
```

Core exports versioned semantic, layout and style contracts without egui, Bevy or kernel types. A future capability publishes `SemanticElement` descriptors with `value_descriptor` and `actions`: typed values, choices, range, units/dimension, accessibility label, diagnostics, command arguments and disabled reasons. Plain fields/actions need no new native view. A specialized view registers a renderer in the desktop `Registry`; an editor-specific value mutation registers a named `ValueAdapters` reader/setter that owns its transaction rules. Readers fetch current values for every placement, so duplicated transient controls do not restore an older snapshot over the active field. Registered field adapters include document-pointer, sketch-dimension, presentation-resource, sketch-block, configuration-dimension and drawing-annotation. Block fields use fresh editor identities; configuration fields resolve stable feature/record IDs; drawing symbol fields resolve stable sheet/annotation IDs and retained staged input. All preserve their owning shared checkpoint and validated Apply path. Adding a capability does not require adding a closed role enum or changing layout parsing.

Portable package schema 1 is `{ "schema_version": 1, "name": "...", "layout": "layout.json", "theme": "theme.css", "active_layout": "instrument-rail", "extends": ["base/presentation.json"] }`. JSON layout/theme resources can also contain `extends` paths; objects merge recursively and arrays replace, while theme rules from package inheritance preserve their cascade order. CSS supports prefix local `@import "base.css";`. Imports are relative to their source, bounded, checked for cycles and observed for changes. Sharing an exported package does not include CAD definitions or an absolute local dependency path.


Fillet and Chamfer maintain an explicit edge set. Command/Ctrl/Shift-click
preselection on one body seeds a new edge feature; while its editor is open,
plain clicks add and modified clicks toggle edges. Add, individual Remove and
Clear use shared draft commands. All authored selected edges remain highlighted
in the viewport's `selection-color`. Tab replaces the current overlapping pick
instead of accumulating each candidate. Editing an existing feature uses its
cached input body so its original edges remain available while the live preview
shows the modified output. Selection controls use stable full provenance
identities; list order is only a presentation label. Edge-editor picks and
highlights retain the complete occurrence path. If repeated shared occurrences
have no unique active path, the first actual geometry pick anchors the editor
before the authored set is highlighted.

The active catalog exposes `edge-selection`, `edge-selection-item` and
`edge-selection-action` roles, with versioned `tcad.presentation.edge-selection`
metadata. `selection.edge-add/<feature>`, `selection.edge-clear/<feature>` and
stable-source `selection.edge-remove/<feature>/<producer>/<role>` commands
serve native controls and custom repeated layouts alike. Saved feature semantics
also publish radius/distance fields and `feature-selection` descriptors. These
controls retain shared checkpoint Cancel and current-evaluation Apply; no
renderer/kernel objects become document source.
