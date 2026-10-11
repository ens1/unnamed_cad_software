# Instrument rail: default layout and theme

![Primary form reference](../assets/ui/instrument-rail.png)

The instrument rail is the bundled default presentation. Implement its structure and appearance through the same user-editable layout/style system available to users. All form guidance below describes this default; users may replace its entire composition and visual treatment.

Theme-agnostic semantic data and commands are specified in [replaceability and presentation](11-replaceability-and-presentation.md). The rail consumes that contract without owning model values, transactions, or interaction semantics. A top toolbar, right panel, floating groups, or another user layout can consume the same information and actions.

## Composition

A continuous vertical rail sits beside a large model viewport. It has four aligned zones:

1. Document and editing context.
2. Model / History / Flow navigation and the current dependency view.
3. The selected operation's parameter instrument.
4. Preview status, Apply, and Cancel.

Use strong horizontal rules between major zones and one strong seam between rail and viewport in this default. These choices belong to layout/style resources; other compositions and control shapes are available to user layouts.

Begin around 300–360 logical pixels wide on a typical desktop; this is a prototype range, not a fixed production width. Make the rail resizable and adapt it to text scaling, many parameters, and complex graphs. Keep pending-edit actions accessible while scrolling.

## Dependency stations

Represent operations as small labeled stations along dependency paths. Use a clear active station ring and one restrained association line to the selected model feature. Labels and hit areas extend beyond the small visible marker.

A simple body history reads vertically. Branches show parameter and shared-definition inputs. A complex graph can expand into a larger canvas, focus a subgraph, or collapse named groups. It must not lose ports, branches, types, or multiple body outputs to resemble a tidy list.

History and Flow are views of one document, not two implementations. The illustrative image's line direction and thickness branch need explicit semantic treatment in the working application.

## Parameter instrument

Show the active operation name, a prominent value, and explicit unit. The large value is editable with precise numerical/expression entry. A scale supplements it; its illustrated range does not cap the actual supported value.

Allow exact entry and controlled trackpad scrubbing. Ordinary scrolling over inactive values does not edit geometry. Multiple parameters use consistent aligned field groups, with the active one clear; do not allocate an enormous decorative gauge to every property.

Display pending, evaluating, valid, and failed states through text and symbols as well as color. Preserve draft text while validation fails.

## Action anatomy

Use a prominent circular Apply control and clearly available Cancel. Provide equivalent keyboard actions and focus behavior. Disable or defer Apply according to the preview contract; never make a green dot imply that an old result is current.

The circular visible shape can have a generous hit area, but it must not overlap nearby controls. Station labels, small ports, and editable values also need forgiving trackpad targets. Start with approximately 40 logical pixels for compact target areas and 72–88 for the visible Apply control, then validate on hardware.

## Selection-first actions

Select a face, edge, vertex, or component first. Show a compact nearby contextual action menu with only valid operations. Choose an action; its source/dependencies and parameters appear in the rail.

Keep the menu near the selection without hiding the geometry or forcing tiny radial hit targets. Command search remains available. The active parameter instrument and 3D handles always address the same draft value.

## Assembly context

Show Editing in place, the active part, and Shared part · 2 instances near the model or in the rail. Keep these annotations compact and unboxed where possible.

Hide/Ghost others, pinned references, temporary assembly peek, and Finish editing remain accessible. Ghost neighbors retain meaningful position and reference identity. Picking a reference uses an explicit mode/action so it does not silently switch the edited part.

## Flat UI, tangible model

Use flat panel fills, strong rules, clean grotesk typography, circular station markers, tabular numerical values, and deliberate spacing. Gradients, glass, UI drop shadows, faux screws, and physical knob decoration are not the form target.

Use realistic material rendering for geometry: coherent brushed aluminum, directional turned-steel detail, roughness variation, blackened steel, rubber, and polymer. Keep microscopic detail restrained and scaled to actual dimensions.

Selection outlines and ghost patches are localized. Committed geometry retains its materials and lighting. Proposed geometry is translucent and can use hatching. Use ordinary laptop-friendly rendering before optional high-end effects.

[design-tokens.json](../assets/design-tokens.json) contains default theme prototype values. The [layout fixture](../examples/drive-unit/ui-layouts.json) illustrates rail and top-oriented compositions; the [theme fixture](../examples/drive-unit/ui-theme.json) illustrates independent CSS-like selectors and variables. None of these resources constrains user positioning, organization, or colors.

Reloading layout/style resources preserves active drafts and semantic selection. Focus and gesture routing follow semantic elements after layout changes. Invalid resources produce runtime diagnostics and retain the last usable presentation; an independent reset mechanism restores the default.
