# Datum targets and feature-control frames

The drawing annotation library includes datum targets and multi-row
feature-control frames. These workflows correspond to the datum-target and
composite-frame authoring described by [Onshape's datum
help](https://cad.onshape.com/help/Content/Drawing/datum.htm) and [geometric
tolerance help](https://cad.onshape.com/help/Content/Drawing/geometric_tolerance.htm),
with datum-target size conventions also documented by [SolidWorks' datum-target
API](https://help.solidworks.com/2024/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IDatumTargetSym~GetTargetAreaSize.html).
TCAD implements a bounded drafting contract; this is not full ASME or ISO
certification.

## Datum targets

Add **Datum target** under drawing Annotations. Its native attachment is edited
with the same model-reference choices and paper reattachment handles as notes and
ordinary datums. Choose a 1–3 letter datum identifier and a target number 1–999.
Letter/number combinations must be unique on a sheet.

Supported markers are an X point, dot point, line, circular area, and rectangular
area. Contact diameter, line length, area width/height, and orientation are saved
expressions with explicit model length/angle units. Circular and rectangular areas
have bounded paper hatching. The divided callout contains the declared size and
datum target identifier; point contact size is optional. Millimeter/inch size
formatting, precision, text size, callout diameter, and hatch spacing are editable.

The marker expresses a requirement in the selected drawing view plane. It does
not infer physical contact, create model geometry, or validate that a declared
contact region lies within a native face. Model geometry or topology changes
still require a current unique attachment. Label movement and elbow leaders use
the existing drawing source and capture rules.

## Composite and stacked frames

Add **Tolerance frame** under Annotations. A composite frame has 2–8 segments
with one shared geometric characteristic. A stacked frame has 1–8 independently
editable segments, each inheriting or overriding the frame characteristic.
Rows retain stable IDs when their order or values change.

Select a segment to edit its positive unit-aware zone value, diameter qualifier,
maximum/least/regardless material condition, and up to three ordered datum
references. Each datum reference has its own material-condition modifier.
Free-state, tangent-plane, and projected-zone qualifiers are supported; a
projected zone retains an explicit positive height expression. Referenced datum
letters must be declared by an ordinary datum or datum target on the sheet.

Switching from stacked to composite deliberately makes the rows inherit the
chosen shared characteristic and supplies a second segment when needed. Add,
remove, and order controls operate within the current drawing draft. Units,
precision, paper text size, and row height are common frame properties. A frame
can be moved, given elbow leaders, or explicitly reattached through existing paper
handles.

The annotation contract stores typed requirements independently of the renderer
and widgets (introduced in drawing contract 9; current drawing contract 10,
GD&T contract 1). New symbols use the same f64 paper vectors for desktop preview, PDF,
and drawing DXF. Editor fields and actions expose stable semantic identities,
source addresses, units, disabled reasons, and commands to user layouts and
CSS-like styles.

Full standards-specific applicability rules, datum simulation, material-boundary
calculation, arbitrary target regions, target coordinate schemes, attachment to
dimension leaders, and certification remain gaps. No kernel geometry or inspection
result is fabricated by an annotation. Compilation, formatting, and strict linting
are the available implementation checks; interactive acceptance remains with the
user, with no tests or UI smoke runs.
