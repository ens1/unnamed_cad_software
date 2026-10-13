# Semantic presentation, layouts and themes

The workspace is a composition of semantic components. Geometry and application transactions do not depend on the layout. The bundled resources are `assets/ui/layout.json` and `assets/ui/theme.json`, with the equivalent editable CSS at `assets/ui/theme.css`. Project packages are saved under `ui/`; personal defaults live in TCAD’s application-support presentation directory. The default layout has conventional CAD command tabs above a left feature tree/property manager and a shared model viewport. The reference images guide the spacious viewport, quiet surfaces, restrained selection cues, and in-place editing; the sample names and geometry are examples, not startup content.

A component has `role`, optional `bind`, optional `command`, and optional `widget` and `label`. Nodes can override these bindings. A field or command binding takes precedence over a native composite role, so replacing an editor does not require privileged widget names. Recognized workspace roles are `document-context`, `command-manager`, `status-bar`, `view-navigation`, `dependency-view`, `property-manager`, `preview-status`, `apply-action`, `cancel-action`, and `model-viewport`. `command-manager` provides Features, Sketch, Assembly, Evaluate, and Graph tabs. Its semantic tool catalog groups modeling under Create, Remove, Modify, Pattern and Reference, and sketching under Draw, Shapes, Curves, Reference and Constraints. `dependency-view` provides the feature tree, numbered History stations, or dependency cards. The graph workspace places cards below the shared viewport; its Assembly scope shows components, mates, and the assembly result. Cards have explicit ports, draggable placement, pan/zoom and contextual edit actions. `property-manager` hosts the active tool editor beside the model; `parameter-instrument` remains a compatible alias. Tool groups can use `assembly-editor`, `materials-editor`, `appearances-editor`, `lighting-editor`, `drawings-editor`, `manufacturing-editor`, `versions-editor`, `review-editor`, `bom-view`, `presentation-editor`, `sketch-editor`, or `library-editor`. A sketch editor provides controls; its geometry always appears in the model viewport. Generic components can bind a semantic entity ID, bind `definition-uuid::parameter-id`, or invoke a registered command. Parameter duplicates edit the same domain parameter and draft.

A layout node can have `id`, `role`, `use`, `layout` (`row` or `column`), `width_logical_px`, `height_logical_px`, `grow`, absolute `position: [x,y]`, `hidden`, `resizable`, `classes`, and `children`. Give duplicated components explicit IDs to retain presentation identity through reordering. Row/column allocation now includes gaps, CSS-order padding insets, min/max sizes, cross-axis alignment, justification, parent-relative absolute positioning and nine anchors. CSS sizes also accept percentages against the parent. `overflow: auto` or `scroll` creates an actual scrolling container; clipping follows the declared hierarchy. Grid, wrapping, media queries and a browser DOM are outside this native subset. Explicit node identities survive reordering; anonymous placement identities use their structural path.

The optional `editing` object overrides `width_logical_px`, `height_logical_px`, and `grow` while the property manager is active. The bundled rail uses it to exchange the tree and tool controls within the same dock. The Model/History/Graph selectors let the user change the rail view; Edit returns to the active controls. While editing, a compact history remains above the scrollable fields. These overrides belong to the layout resource and can be changed or removed by the user.

Modeling editors use the common validated preview transaction. The compact orange Apply action waits for the matching evaluation and commits one undoable change. The sketch editor remains open while validation is pending or fails, retaining fields for correction. Further edits cancel the queued confirmation and update the preview. Cancel restores the session checkpoint taken when the operation started, retaining any edits that predated the operation. Existing Apply/Cancel command bindings remain available for custom layouts and general document edits.

Operation previews draw one neutral evaluated result with localized added and
removed material. An additive extrude highlights its new volume; cuts, chamfers,
and other subtractive edits highlight the removed volume. Existing feature
editors compare that feature's current evaluated input and output, so the old
feature and downstream geometry do not become the comparison baseline. Opening
an existing feature requests this display cache even before changing a value.
The rest of the result retains its visual finish, without a second complete
committed body behind the draft.

Highlights belong to the displayed evaluation, its feature target, and its
baseline identity. Pending or invalid changes retain matching last-valid
display geometry; it never becomes eligible for Apply. Original and History
views hide operation highlights. Explicit revision comparisons remain a separate
view. Removal caches do not participate in topology picking, property
integration, modeling, or export. Fit includes localized removed geometry.

The worker prefers native B-rep material differences and uses bounded
tessellated differences when the native display comparison is unavailable. A
display approximation is labeled in the status bar, with details on hover.
Unsupported or oversized comparisons leave the valid result neutral and show a
localized-highlight diagnostic; they do not invalidate the CAD operation.
Surface creation/deletion and unchanged native faces have localized caches;
overlapping edited surface trims currently report unavailable. Generic
checkpoint comparisons describe local shape changes, rather than occurrence-only
assembly movement.

Style additions and removals independently on the viewport:

```css
[role=model-viewport] {
  preview-added-color: #4ECF8880;
  preview-removed-color: #FF8A4A80;
  preview-edge-width: 1.25px;
}
```

Color alpha controls the translucent overlay. Existing `preview-color` remains
the fallback for styles that omit these properties. Individual overlays also
support semantic child selectors under the actual viewport ancestry:

```css
[role=model-viewport] > [role=operation-preview-added] {
  color: #4ECF88;
  opacity: 0.5;
  stroke-width: 1px;
}
[role=operation-preview-removed] { color: #FF8A4A80; }
```

Use `display: none` or `visibility: hidden` to hide a selected overlay role.
These controls affect rendering and Fit, without changing the CAD operation.
Semantic
`workspace.operation-preview` and `operation-preview:<mesh-key>:added|removed`
descriptors expose the target, availability, baseline kind, representation,
tolerance, and retained/unavailable diagnostics without palette or widget data.
The application-owned operation-preview contract is version 1.

History stations now invoke the view-only `history.inspect` command. A matching valid evaluation supplies cached intermediate body meshes and sketch visibility at each feature. Clicking a station replaces the displayed active part with that stage; other parts retain their latest geometry and assembly placements/configurations remain intact. Later rows are dimmed. Mesh rendering, picking, selection outlines and Fit use the same cached stage, so invisible later faces and sketches cannot be selected. The cursor does not change definitions, outputs, suppression, draft or undo state, and switching stations never submits an evaluation request. **Latest model** in the rail, **Latest** in the viewport, the `history.latest` command, or Escape from the viewport restores the current display. Feature editing leaves inspection before opening its normal editor; geometry modeling actions explain that Latest must be restored first.

History is feature-sequence inspection using current parameter values, separate from Git revision comparison. A pending/failed evaluation, missing/incompatible cache or non-causal feature order reports an explicit reason instead of rebuilding on click or substituting final geometry. Normal evaluation prepares the caches once. Inspection cannot start during an active modeling operation. The cursor is transient and saved projects reopen at Latest. Semantic `history-station` IDs are `history:definition-id::feature-id`; they expose the versioned target, index, availability and disabled reason. Bind `history-view` for the inspection label and Return to Latest action in a custom layout.

The Size menu controls UI scale (0.75–1.5) and toolbar height (60–130 logical pixels), with Compact and Comfortable presets. Toolbars scroll horizontally; tree, history and controls scroll vertically. These controls edit presentation state/layout without changing the source model. `ui/workspace.json` stores UI scale, toolbar setting, graph scope, graph positions, pan/zoom, sketch annotation positions and dimension/relation visibility. Save presentation writes the current resized layout and state. Arbitrary saved layout heights are respected until the user explicitly changes sizing.

The viewport has a clickable view cube and leaders for the selected feature/value or geometry measurement. Pinned assembly component references have separate labels; edit-in-place offers Ghost others, Hide others and Finish Editing in the viewport. These views have independent registered renderers and semantic bindings, in addition to their default viewport overlay placement. New documents stay empty and use the user's own part names.

Click an edge, face or sketch geometry in the model viewport to open a compact action popup beside the selection. Primary clicks in the tree, History, graph, property manager and semantic selection controls select or inspect without opening a popup. Clicking empty viewport space clears the selection without opening actions. Feature selection and popup opening are separate presentation operations; double-click editing and explicit right-click menus remain available. Edge actions include Fillet and Chamfer. Faces offer Sketch on Face, Shell with that face removed, and Draft; Sketch on Face explains when the selected face is not planar. Sketches offer boss/base and cut operations; solid features offer body modifiers and patterns. Edit, Suppress/Unsuppress, and Delete target the selected feature. Geometry actions prefill the existing modeling editors and keep the common validated Apply/Cancel transaction. While an operation is active, geometry clicks continue selecting its inputs and expose applicable view commands; competing modeling operations have disabled reasons. Active sketches project their own member/point/reference/constraint actions. Clicking outside, navigating, starting an action, or pressing Escape dismisses the popup while retaining the selection. Double-clicking a sketch or feature opens its editor.

Sketch members and endpoint/center handles in the model viewport support the same primary-click action popup. Member, point, reference and constraint controls in the property manager only select their target; double-click a dimension row to edit its value. Shift/Command-click adds or removes an entity; Origin, X Axis and Y Axis selectors make fixed sketch references accessible even when geometry overlaps them. Relations shown in the popup or property manager are filtered by entity types. On X/Y Axis aligns an entire straight line, or places a curve start/center or point on the axis. Coincident with Origin anchors a member's start/center; selecting an endpoint explicitly determines which point is anchored. References are the active sketch's plane-local frame, including for face-attached sketches.

Smart Dimension is available in the Sketch command tab, selection popup, command search and **D** shortcut. Select a member or Shift-select two points/lines, then invoke it; an empty selection starts a pick sequence. The property manager offers applicable dimension types and a live expression field. For a point pick sequence, click the second entity or press D again for coordinates from the origin. Choose **Create Named Parameter** to bind the dimension to a new D1/D2/etc. parameter, then edit it under Named Parameters or another parameter binding. Sketch and downstream geometry previews update before Apply; invalid/conflicting constraints retain the last valid preview and block confirmation. Cancel restores the operation checkpoint, including newly created dimension parameters.

Click a dimension/relation label to edit or remove it through the popup; double-click a dimension to open its value editor. Drag labels to change presentation-only positions, or hide Dimensions/Relations in the property manager. Style annotations with `[role=sketch-annotation]`, `.dimension`, `.relation`, or `#constraint-id`; supported properties include color, background, font-size, font-family, padding, border-radius, display, and initial left/top offsets relative to the geometry anchor. Saved dragged offsets override initial offsets. `--sketch` sets the default sketch geometry color. Semantic `sketch-member`, `sketch-point`, `sketch-constraint`, and `sketch-reference` bindings expose the same selection and editing commands independently of annotations. Native controls remain docked.

In Select / Move, sketch handles drag endpoints, arc through-points, centers and spline controls. Selected members expose diamond move handles; circles expose square radius handles, and ellipses expose square major/minor radius handles plus a rotation handle. A selected spline shows its control polygon. Center rectangles include a center point related to a construction diagonal. New rectangles infer horizontal/vertical relations but do not automatically fix position or size. Add Smart Dimensions or origin/axis relations to constrain those freedoms.

The cursor is a temporary solver goal: dragging moves related geometry while preserving engineering constraints. Fixed positions and dimension/parameter-driven size handles show locked styling and explain their driver. Alt bypasses grid snap. Each gesture adds one sketch history entry; Escape restores that gesture's starting document and preview without discarding prior pending edits. Leaving the viewport freezes movement; releasing keeps the result pending, and loss of window focus cancels capture. Navigation is suspended during capture. Apply still requires the current valid worker evaluation, and Cancel restores the sketch operation checkpoint. Invalid drag candidates retain the valid sketch. The click menu's Drag Handles action preserves selection and returns to Select / Move.

Semantic `sketch-handle` elements carry version 1 handle targets, stable IDs, plane-local f64 positions, labels and disabled reasons. `sketch.select-handle` selects the owner and exposes the native drag controls; other renderers can submit goals through the core handle/solver contracts. Style `[role=sketch-handle]` with `.point`, `.center`, `.control`, `.move`, `.size`, or `.rotation`; states are ready, hovered, selected, locked and dragging. Supported properties are color, background, size, hit-size and display. Native sizes are clamped to 4–30 logical pixels with a larger hit target; these appearance values never change source geometry.

Part/component rows, named parameter labels, material labels, planes, Origin, and assembly graph objects also publish relevant contextual actions for explicit commands and docked action bindings; primary selection does not open a menu. Parameter edits use the same identities and shared draft during sketching; they do not switch away from the active sketch editor.

Selection capabilities are published by `cad-core::selection_actions` as `selection-action` semantic elements, with command identities, target references, groups, availability, and disabled reasons. Bind a custom component to `selection:feature.chamfer` or another published selection action to present the same command elsewhere. The descriptors appear in the binding catalog when an entity is selected. A `selection-actions` layout role displays the current selection's actions in a dock of your choosing. The default floating presentation has ID `selection-popup`; style its width, height (scroll limit), padding, gap, font size, colors, border, radius, and position through theme rules. `#selection-popup { display: none; }` disables the floating menu if you prefer a docked action list.

[Multiple-entity sketch relations](sketch-bulk-relations.md) publish their complete
selection count and selection-semantics description in the existing action
descriptors. Cmd-click picks keep the floating menu closed, while right-clicking
a selected entity retains the complete group for contextual actions. Spatial
relation actions use the same descriptive metadata and explicit disabled
reasons; inherited circle radius fields expose their actual reference and
driving dimension through read-only field metadata. These contracts remain
independent of control placement or theme rules.

Planar sketch reference and relation controls use the shared button adapter,
with roles `sketch-reference-action` and `sketch-relation-action`. Their stable
IDs are `<sketch>::reference-action.<reference>` and
`<sketch>::relation-action.<kind>`. The editor publishes their actions, selection
state, descriptions and `modeling-action` / `compact` classes for custom layouts.
Both bundled themes give these buttons filled surfaces and visible borders;
height, pointer targets, padding, colors and shape remain theme properties.
Reference buttons also show their selected state. Shared buttons publish
transient `hover`, `active` / `pressed`, and `focus` states for pointer and
keyboard feedback. Reference rows wrap with the available panel width.

The planar Tangent action remains published and visible when its selection is
incomplete or incompatible. Its disabled value/metadata explains the required
selection, including ambiguous point ownership. Endpoint/center promotion and
contact intent belong to the shared sketch planner rather than a widget-specific
selection rule. See [Tangent selection](sketch-bulk-relations.md#tangent-selection).

Saved complete theme packages retain their authored styling. Selecting the
bundled Classic or Nocturne entry in Settings loads the current bundled rules;
user resources are not overwritten to apply a visual update.

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

Native CAD editors remain available as registered views. Their document fields can be replaced or duplicated through stable bindings. Toolbar actions, command tabs, individual menus and entries, feature/history rows, selection actions and confirmation controls use the shared styled-control adapter. Smart Dimension value/reference fields, resource source/watch fields, sketch block source/placement, configured feature dimensions, and drawing fields publish transient setters. Viewport context, start hint, view cube, selection candidates, isolation controls and navigation hint have stable styling identities and parent-relative sizing/anchors. Surface, direct-face, sheet-metal, structural-member, mold, fillet, chamfer, shell, draft, pattern, transform, scale, Boolean and mirror editors render individual fields, reference choices, source lists and actions through shared semantic controls. Drawing sheets, views, formats, dimensions, annotations and tables use that same descriptor renderer. Extrude, Revolve, Loft, Sweep, reference geometry, Hole, Rib and other remaining feature editors, plus some geometry-attached overlay internals, still use composite adapters. Native file pickers remain operating-system dialogs. Duplicate model viewports currently share one camera and render texture.


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

Core exports versioned semantic, layout and style contracts without egui, Bevy or kernel types. A future capability publishes `SemanticElement` descriptors with `value_descriptor` and `actions`: typed values, choices, range, units/dimension, accessibility label, diagnostics, command arguments and disabled reasons. Plain fields/actions need no new native view. A specialized view registers a renderer in the desktop `Registry`; an editor-specific value mutation registers a named `ValueAdapters` reader/setter that owns its transaction rules. Readers fetch current values for every placement, so duplicated transient controls do not restore an older snapshot over the active field. Registered field adapters include document-pointer, sketch-dimension, presentation-resource, sketch-block, configuration-dimension, operation-editor, drawing-editor and drawing-annotation. Block and drawing editors use fresh session identities; configuration fields resolve stable feature/record IDs; drawing fields resolve stable sheet/item IDs and retained staged input. All preserve their owning shared checkpoint and validated Apply path. Adding a capability does not require adding a closed role enum or changing layout parsing.

Portable package schema 1 is `{ "schema_version": 1, "name": "...", "inherit_defaults": false, "layout": "layout.json", "theme": "theme.css", "active_layout": "instrument-rail", "extends": ["base/presentation.json"] }`. Complete exported packages use `inherit_defaults: false` so selecting, saving, and reloading preserve the same compositions and style cascade. Legacy manifests that omit this field remain patches over the bundled presentation. JSON layout/theme resources can also contain `extends` paths; objects merge recursively and arrays replace, while theme rules from package inheritance preserve their cascade order. CSS supports prefix local `@import "base.css";`. Imports are relative to their source, bounded, checked for cycles and observed for changes. Sharing an exported package does not include CAD definitions or an absolute local dependency path.

**Settings → Application → Themes** previews bundled and personal library choices. Load a complete manifest to replace both layout and styling, or load a CSS/JSON theme to retain the current layout. Apply saves the selection as the personal presentation default; Cancel restores the presentation and its reload sources. The [Nocturne package](theme-example.md) demonstrates alternate docking, control sizes and shapes, colors, typography, and viewport overlays using editable resources. The native picker publishes `settings:field:presentation:theme` and registered settings actions, so a custom composition can host the same controls.


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

Visual finish and studio-lighting editors publish contract-v1 `appearance-field`, `appearance-assignment`, `occurrence-appearance` and `lighting-field` descriptors. Native controls use their stable IDs and setters; custom layouts can repeat or bind the same fields/actions. Appearance input controls, tabs and source text also publish transient setters. Engineering material editing remains separate. Viewport styles additionally accept finite numeric `environment-intensity`, `key-light-intensity`, `fill-light-intensity` (0–20), `environment-rotation` (degrees, ±1000000), `exposure` (stops, −10–10), and `ambient-occlusion-radius` (render units, 0.001–10; one unit is 100 mm), plus `shadows: true | false | none`. These presentation overrides leave saved document lighting unchanged. See [visual finishes](visual-finishes.md) for mapping, relief, configuration assignments and limits.

The dedicated `workspace.appearance-panel` view uses role `appearance-panel`
and semantic `open`/`closed` state. Bundled compositions dock it beside the model
viewport with user-resizable width; closed panels release their layout space.
Users may move, restyle, duplicate or replace it through the ordinary composition
contract. Existing custom compositions that omit it retain the property-manager
appearance editor. Explicit target part/scope/instance fields use transient
`appearance-ui` setters. Per-target customization duplicates and assigns a
finish through the shared draft; editing a library finish requires the explicit
shared-edit action. All import/clear actions still publish the finish and map
slot independently of their widgets. See [visual finishes](visual-finishes.md).

The Settings dialog exposes `settings:dialog`, `settings:category:<category>`, and `settings:field:<scope>:<pointer>` descriptors with typed values, ranges, choices, stable setter `settings`, and scope/editability metadata. Settings fields can be bound by user compositions; edits open the same scoped transaction as native controls. Search is a transient `settings-search` field. `settings`, `settings.model`, `settings.application`, category-opening commands, and Apply/OK/Cancel are ordinary actions. The default modal uses `settings-dialog`, `settings-category`, `settings-field`, `settings-heading`, `settings-description`, and `settings-action` style roles. CSS width/height, left/right/top/bottom and percentage lengths control modal sizing and placement; without explicit dimensions the native surface can be resized. Application and model data remain independent of these visual choices. See [settings](settings.md).


## Modeling controls and independent viewport views (2026-10-11)

Native set selection, the sketch relations manager and explicit reference repair
use shared commands and presentation descriptors. See
[everyday modeling controls](modeling-workflows.md) for exact contracts, available
source types, solver feedback and geometric limits.

Bindings to `viewport.toolbar`, `viewport.start-hint`, `viewport.view-cube`,
`viewport.candidates`, `viewport.isolation` and `viewport.navigation-hint` render
these controls in ordinary user layout regions. Their original overlay remains
separately hideable with CSS. `workspace.model-context`,
`workspace.selection-summary`, `workspace.candidate-summary`,
`workspace.navigation-instructions`, `workspace.validation-state` and
`workspace.status-message` expose their current text independently. Native buttons
use `workspace-action` identity and styling. An Actions command is unavailable
while an active editor owns the shared checkpoint.

The default toolbar uses 28 px ready and 56 px selected/editing/inspection heights;
selected history rows retain their 26 px height and feature-tree rows use 24 px. Users can override all of
these values. Viewport overlay positioning now resolves the current state as well
as its role/ID. Compact row text can use the portable `text-overflow: ellipsis`
property while preserving complete semantic/accessibility and hover labels.
`text-overflow: clip` retains clipped text without an ellipsis; other values
are rejected by theme validation. Both CSS and JSON declarations use this
property, including Classic Orange and presentation recovery. Button `max-width` and
`max-height` accept percentages relative to the actual available region.

`<sketch-id>::feedback` and `reference-repair-manager` bind directly to the
`sketch-feedback` and `reference-repair` panel renderers. Their individual filters,
constraints, fields, failures, upstream stages and replacement choices can also
be composed using ordinary semantic bindings. No alternate document, solver or
undo state belongs to these renderers.


## Semantic modeling editor families (2026-10-11)

Surface, direct-face editing, sheet-metal, structural/weldment, mold, Fillet,
Chamfer, Shell, Draft, Pattern, Transform, Scale, Boolean and Mirror feature
editors publish contract-v1 `tcad.presentation.operation-editor` descriptors. The
native property manager renders these descriptors; it no longer owns a separate
set of field values or privileged source-selection buttons for these families.

`operation-editor-field` provides exact expression or f64 entry, Boolean options,
and labeled choices for sketches, bodies, surface inputs, datum/origin references,
selected geometry, section points and manufacturing members. Ruled-surface
directions expose their explicit, axis, plane-normal and two-point definitions;
associated references retain their source intent. `operation-editor-action`
provides add/remove/reorder for loft/knit/ruled inputs and explicit sweep stations,
face/edge selection, structural member sets and analyzed parting edges. Stable
source/segment/topology identities resolve against the latest staged feature
before collection mutations. Explicit sweep station coordinates remain ordered
position slots, matching the saved feature contract.

`operation-editor-group` and `operation-editor-help` expose native grouping and
assessment text independently of the fields. Groups include `sources`,
`selection`, `extent`, `correspondence`, `placement`, `members`, `bend`, `pull` and
`assessment`; each group name is also a CSS class. Consecutive list actions use
compact wrapping rows. Group `overflow: auto | scroll` and `max-height` create a
bounded scrolling list in the native view; the containing dock's ordinary layout
still controls its size.

A custom composition can move or duplicate all active modeling fields without
adding a native widget:

```json
{
  "id": "my-modeling-fields",
  "role": "semantic-field-collection",
  "layout": "column",
  "overflow": "auto",
  "repeat": { "role": "operation-editor-field" },
  "template": { "role": "field-control", "widget": "field", "height_logical_px": 44 }
}
```

Use a second collection with `repeat.role: operation-editor-action` and
`template.widget: action` to place the source-selection and list actions. Fields
use stable identities such as
`operation-editor:<feature-id>/operation/operation/distance`; arrays with authored
segment IDs or referenced source IDs expose those identities in their collection
bindings instead of depending on a stale widget index.

```css
[role=operation-editor-field] { font-size: 11px; }
[role=operation-editor-action] { height: 22px; padding: 3px 6px; }
[role=operation-editor-group].assessment { overflow: auto; max-height: 160px; }
[role=operation-editor-help] { font-size: 10px; }
```

Each setter/action carries the current document, part, feature and editor-session
identity. Readers fetch current source for every placement, and setters invoke
the same `PutFeature` transaction and asynchronous evaluation as the native
property manager. A stale editor cannot modify a later session. Changing a source
body or path reconciles its dependent face/edge/member selection rather than
retaining references to the previous source. Optional bend radius and K-factor
overrides preserve their entered expression until disabled.

Invalid partially typed numeric input and rejected typed edits retain their field
text and diagnostic after focus moves. They prevent Apply/OK even while a previous
geometry evaluation remains valid; correcting the field restores normal current
validation. Removing the relevant field or canceling the editing session discards
its local invalid input. Advanced Source retains its separate JSON validity path.
These presentation changes extend control composition and editing safety; they
do not broaden the native geometry support of the underlying operations.

Numeric constant controls accept explicit compatible units, including `mm` and
`deg`, without changing the saved f64 unit convention. Omitted optional source
fields keep their omission until edited. Field `flex-direction: row | column`,
`width`, `height`, `min-width`, `max-width`, `min-height`, `max-height` and
`min-target-size` control native value placement and size. Choice menus provide a
search field and virtualize long catalogs; compact labels retain full hover text.

## Semantic drawing inspector (2026-10-11)

The drawing inspector publishes contract-v1 `tcad.presentation.drawing-editor`
descriptors. Sheet, Controls, Item and Row selectors expose the current context;
selecting a different context changes which individual fields and actions appear
in the catalog. The native inspector uses the same grouped descriptor renderer
as modeling editors. A custom composition can relocate or duplicate the current
controls without creating a second drawing draft.

`drawing-editor-field` covers sheet names and model scopes, paper dimensions and
presets, view selection/order/placement/scale, sheet format borders and zones,
title-block cells and linked text, dimensions and tolerance options, leader
notes, datums, geometric tolerances, center marks/lines and BOM balloons. Model
point, curve and native hole references retain their complete occurrence paths.
Existing surface-finish and weld-symbol bindings keep the
`drawing-annotation-field` role and join the same staged inspector.

All six supported table types expose ordinary controls: BOM, revision, general,
bend, hole and cut-list. Controls include placement, text and row sizing, column
widths, general headers/cells, revision rows, source scopes/configurations,
source selections, hole datums and saved item numbers/order. Source-row controls
remain available for correcting saved numbering even when table-content
validation rejects that numbering. These controls expose existing native
drawing capabilities; they do not add new projected-view or annotation geometry.

`drawing-editor-action` supplies sheet/template operations, view toggles and
ordering, annotation/dimension/table creation and deletion, title-cell and table
row/column editing, source selection, reservation refresh, Apply/Cancel and
PDF/DXF export. `drawing-editor-group` and `drawing-editor-help` expose groups and
diagnostics; group names are CSS classes. Sheet, view, format, dimension,
annotation and table groups remain independently scrollable through
`overflow: auto | scroll` and `max-height`.

```json
{
  "id": "my-drawing-fields",
  "role": "semantic-field-collection",
  "layout": "column",
  "overflow": "auto",
  "repeat": { "role": "drawing-editor-field" },
  "template": { "role": "field-control", "widget": "field", "height_logical_px": 36 }
}
```

Use additional collections for `drawing-annotation-field` and
`drawing-editor-action` to include symbol controls and commands. Direct bindings
use IDs such as `drawing-editor:<sheet-id>/paper_mm/0` and
`drawing-editor:<sheet-id>/tables/@<table-id>/position_mm/0`.

```css
[role=drawing-editor-field] { font-size: 10px; flex-direction: row; height: 24px; }
[role=drawing-editor-action] { height: 22px; padding: 3px 6px; }
[role=drawing-editor-group].table-columns { overflow: auto; max-height: 180px; }
[role=drawing-editor-help] { font-size: 10px; }
```

Readers and actions validate the current document, sheet and editor-session
identity before resolving source. Authored dimensions, annotations, title cells,
tables and revision rows retain their saved IDs. Anonymous general-table rows,
columns/cells and sheet notes use session slot identities that survive native
insertion and removal; these identities do not alter the document format.
Invalid partial text remains staged after focus or inspector context changes and
prevents Apply/OK until corrected or its source field is removed. Typed source
that fails document validation stays available for correction without committing
it. All accepted changes enter the existing `PutDrawing` draft/checkpoint;
current validated evaluation remains required for Apply, and Cancel restores the
starting document. Native file/template pickers retain their operating-system
presentation.

## Part edges and hover center guides

Parts default to **Shaded with edges**. The desktop retains native boundary line
assets, scoped to the same displayed bodies, assembly occurrences and history
station as the shaded model. Their depth test suppresses hidden edges; a small
depth bias keeps front boundary lines from flickering on the surfaces. Default
width is one logical pixel, scaled for the viewport's render target. Native
selection outlines retain their existing accent above these neutral lines.

Personal **Settings → Application → Model display** toggles edges, circular
centerlines and straight-edge midpoint guides. All default to true. A theme may
explicitly override the corresponding viewport visibility properties:

```css
[role=model-viewport] {
  show-part-edges: true;
  part-edge-color: #333A44;
  part-edge-width: 1px;
  hover-centerlines: true;
  hover-midpoints: true;
  centerline-color: #526B87;
  centerline-width: 1.25px;
  centerline-hit-size: 12px;
  centerline-extension: 24px;
  centerline-mark-size: 7px;
}
[role=part-edge][definition=part-1] { stroke-width: 1.5px; }
[role=hover-centerline] { color: #FF5A00; opacity: 0.8; }
[role=hover-midpoint] { size: 5px; hit-size: 16px; }
```

Bundled themes set suitable colors and leave visibility preferences in control.
The new visibility properties require true/false. Their width, hit, extension and
mark properties accept nonnegative pixel lengths (or unitless zero). Widths are
bounded to 0–8 logical pixels, proximity to 0–64, extension to 0–200 and marks to
0–40; zero width/alpha or `display: none` hides an overlay. Semantic role selectors
also support color, opacity, stroke-width, hit-size, size and visibility. Edge
IDs retain encoded occurrence, body and construction identities; attributes
include definition, occurrence, producer, body and construction-role.

Hovering near a proven circular rim or its center reveals a center mark, in-plane
centerlines and normal axis. Hovering near a proven straight edge reveals its
geometric midpoint and a perpendicular guide. These are noninteractive feedback;
they do not open menus, add geometry, change the sketch or commit a draft. They
respect occurrence transforms, the history station and edge-feature input
selection. Opaque foreground occurrences still occlude input guides while editing
an edge feature. The pointer leaving the viewport, drawing capture, navigation,
component dragging or popup ownership clears feedback.

The application-owned `EdgeData.center` display contract version 1 supplies
mesh-local f64 endpoints for proven bounded straight carriers, or center, normal,
radius and radial direction for proven circular carriers. Circle recognition uses
complete native rational identities, including supported arcs; screen samples
only locate nearby edges and assess occlusion. Elliptical/spline/implicit carriers
without a proved supported center omit this metadata. Saved source and the native
boundary are unchanged. Adapter 31 invalidates older derived caches so this
optional metadata is regenerated.

`workspace.edge-display` publishes effective display settings through contract
`tcad.viewport-edge-display` version 1. Current guides publish
`tcad.viewport-guides` version 1, roles `hover-centerline` / `hover-midpoint`,
construction references, complete occurrence paths, native center metadata and
transient visible/hover state. They carry no modeling command. The center-guide
renderer remains replaceable independently of the native kernel and CAD source.

Hover queries cache both hits and misses until the pointer, viewport clipping,
evaluation, feature-input selection, camera, history/exploded view, isolation,
theme or relevant preferences change. Each cache miss shares a 200,000-segment
proximity budget and a 200,000-triangle occlusion budget across up to eight nearby
candidates. Budget exhaustion conservatively omits optional feedback. Native
center recognition remains in the geometry worker; cached guides retain only one
analytic frame, occurrence, styling and semantic description. Transient semantic
snapshots reject changed evaluation or view contexts. Duplicated custom viewport
regions do not clear the pointer-owning region's cache.


Drawing paper editing publishes `drawing-interaction-field` and `drawing-paper-item` descriptors and source-owned handles. `drawing.interact-*` commands and the `drawing-interaction` value adapter resolve current document/sheet/page/editor identity before mutating source. Production controls publish their own semantic values/actions, with configured BOM/stock content independent of widget placement. Paper camera zoom/pan and selection are disposable presentation state; paper dimensions, printed scales, alignment relationships and leader offsets are drawing intent. See [paper interaction](drawing-paper-interaction.md) and [production controls](drawing-production.md).

### Guided structural members

The `tcad.presentation.structural-member` v1 descriptors expose layout sketch, exact member selection, namespaced catalog/collection/type/size, locating point, rotation, signed section X/Y offsets and group/end controls. `structural-member` is the registered value adapter; `structural-member.action` handles picking, bulk selection, library import/export, restoration and explicit custom conversion. The default property manager groups these descriptors into Sketch entities, Pre-made profile, Locate profile and Group and ends. Users can compose the same fields/actions independently.

`tcad.presentation.structural-profile-locator` v1 supplies contours, actual remapped point IDs and placement expressions. The `structural-member-locator` view adapter and semantic binding render its disposable diagram; the `structural-member-editor` adapter renders the complete default editor. Diagram and path roles consume CSS-like colors, sizes and stroke properties. They never supply kernel geometry or saved placement authority. See [structural member profiles](structural-member-profiles.md).

## Spatial sketch presentation

The `tcad.presentation.sketch3d` v1 descriptors publish groups, fields, notes and actions with document/owner/feature/editor/source identity. Roles beginning `sketch3d-` are individually styleable; the `sketch3d` value adapter and `sketch3d.action` dispatch edit the same native draft as the docked `sketch3d-editor` view. Drawing plane, grid, input coordinates, entity selection, dimensions, relations and diagnostics remain distinct semantic information. Viewport point/member/axis handles use disposable projection of f64 source and worker evaluation; camera or renderer objects are not source authority. See [3D sketch editing](sketch3d.md).

## Inspector group hierarchy

The default tree and property manager separate related controls with full-width
section headings and a thin divider. The compact orange theme uses flat, shaded
headers; Nocturne gives the same sections rounded purple headers. Section bodies
remain unboxed, so the grouping does not introduce repeated cards or larger
modeling controls. Sections can collapse independently; this is disposable
presentation state and never changes the model or its draft.

Existing groups retain their semantic IDs, roles and commands. The shared native
renderer adds the `inspector-section` class, a child
`inspector-section-heading` at `<group-id>/heading`, and a child
`inspector-section-divider` at `<group-id>/divider`. The header and divider are
independent style targets. Existing `.modeling-group`,
`operation-editor-group`, `structural-member-group`, `sketch3d-group`,
`drawing-editor-group` and the tree's `feature-tree-group` consume the same
hierarchy styles. User layouts can continue to move or duplicate the original
groups and their bound controls.

Both bundled themes expose reusable `--section-*` variables for body and header
backgrounds, border color, padding, gap, header height, corner radius, and divider
width/spacing. These are ordinary theme variables, not reserved engine options.
Individual selectors can override the variables or hide either decoration:

```css
:root {
  --section-heading-background: #E5E2DA;
  --section-border: #B5B3AE;
  --section-heading-height: 24px;
  --section-gap: 4px;
  --section-divider-spacing: 6px;
}
[role=inspector-section-heading] { border-radius: 0px; }
[role=inspector-section-divider] { stroke-width: 1px; }
[role=sketch3d-group] [role=inspector-section-heading] { color: #0087BE; }
```

Heading styles control its background, text, padding, size, border and shape.
Divider styles use `color`, `stroke-width` and `height`; `display: none` or
`visibility: hidden` suppresses a decoration. Container paint stays on the
section frame and does not recolor the input fields or buttons it contains.
