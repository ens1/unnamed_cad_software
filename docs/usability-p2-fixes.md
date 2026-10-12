# Usability P2 implementation — 2026-10-12

This implements the six P2 findings in the 2026-10-11 usability report, plus its
recommendation to keep operation completion controls visible. The original sample
project and PDF report remain unchanged. Interactive acceptance of these fixes
remains with the user.

| Finding | Implementation |
| --- | --- |
| Context menus bury everyday commands | Faces lead with Sketch, Hole and common face edits; edges lead with Fillet and Chamfer. Secondary actions use compact semantic flyouts. Hidden actions cannot leave empty groups. Titles and controls fit the popup width. |
| Smart Dimension and hole positioning require repeated handoffs | A first line/entity pick opens a nearby editable dimension, with a second reference still available. Done Dimension/Return finish that dimension. Hole positioning retains the operation prompt, carries deliberately placed center IDs forward, and opens the hole feature in the same checkpoint. Finish Positions and hole OK remain distinct. |
| Component creation and positioning are unclear | New Part in Assembly, Insert Existing, Move and Mate are separate actions. Existing definitions and linked local projects use one replaceable insertion preview in the selected owning assembly. Six semantic placement fields identify X/Y/Z, units and parent assembly coordinates; component actions wrap inside narrow panels. |
| Invalid welds lack useful feedback | The editor explains required seams/supports and shows joint orientation. Current native failure results retain seam/root/toe guides, support identity and contact errors. Failed contacts are marked in the viewport; irrelevant roll/vector fields are hidden for face-supported joints. Replacement support picks use the validated operation checkpoint. |
| Debug information appears on drawing output | Ordinary view labels and standard scales remain on paper. Fidelity/provenance descriptions, source hash and renderer details move to semantic Drawing Information. The same clean vectors feed preview, PDF and drawing DXF. |
| Drawing controls lose paper context and require scrolling | Drawing is a named workspace with a persistent sheet/view/annotation tree, grouped sheet creation controls and pinned Apply/Cancel/export controls. Apply retains paper, sheet, selection, page and navigation. Saved presentation preferences restore paper when reopening the project. Modeling commands require leaving or finishing the drawing edit. |

## Presentation and transactions

Selection action contract 4 adds default `data.menu` hints: group identity,
ordering, group ordering and primary/flyout membership. Command identities and
availability stay independent. Themes can change native popup ordering with
`renderer-menu-order`, promote an action with
`renderer-menu-presentation: primary`, or render a group inline with
`renderer-menu-presentation: inline`. Custom layouts can bind and regroup the
same action identities. These renderer extensions do not alter model source.

The default menus and new sketch/component/weld controls use semantic roles,
classes, states and ordinary style properties for sizing, spacing, colors and
visibility. `drawing-tree` and `drawing-completion` are replaceable composition
components; [drawing workspace details](drawing-workspace.md) document paper
state and information bindings.

Completion labels expose their scope: Done Dimension, Finish Positions,
Finish Sketch, OK Feature, OK Component, OK Mate, Apply Assembly and Apply
Drawing. Modeling changes share the existing validated draft and undo path.
The hole handoff preserves its starting checkpoint until the hole's OK/Cancel.
Component file insertion keeps the linked revision and resource bytes, removes
the link loader's temporary root occurrence and inserts one stable preview into
the intended parent. Empty, failed or invalid placement edits cannot Apply.

## Native diagnostics and limits

The backend is `monstertruck/0.4.1+fc2306852a46;adapter/36;solver/tcad-native-lm/16`.
Adapter 36 invalidates older derived caches and adds disposable weld-joint
preview contract 1 to the worker evaluation. No Monstertruck fork changes were
required. The guides contain f64 points, transforms, support/source identities,
orientation and contact-validation state. They are display information and never
authorize a native bead or manufacture replacement modeling references.

A free plate edge without a suitable joint remains an invalid face-supported
weld when its toes leave the selected trimmed supports. The editor explains the
failure; it does not substitute a successful bead. Unsupported weld carriers and
corner transitions retain native limits.

Drawing hidden-line removal, arbitrary sections, standards libraries and general
topology repair are unchanged by this usability work. Fidelity/repair status
stays explicit in Drawing Information even though implementation descriptions no
longer appear on production paper by default. Export requires current validated,
applied drawing source.
