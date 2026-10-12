# Drawing dimension groups

Production drawings now support saved **ordinate, baseline, and chain** groups in
the `front`, `top`, and `right` orthographic views. These are drawing annotations,
not sketch constraints. Their coordinate values follow the evaluated mechanical
model and do not drive its geometry.

In the drawing inspector, add **Dimension group** under Annotations. Choose its
style and projected direction, then choose a common datum and ordered targets.
Existing linear point dimensions can supply the initial datum and target. A new
group without those references starts with clearly declared explicit coordinates;
reattach them with the native reference choices or paper attachment handles.

- **Ordinate** reports signed X or Y coordinates from the datum and includes a zero
  datum marker. Labels retain their own paper offsets.
- **Baseline** measures every target from the common datum and spaces successive
  dimension lines by the saved paper spacing.
- **Chain** measures each target from the preceding ordered target, starting at
  the datum. Earlier/Later controls change that sequence explicitly.

Each group contains 1–128 targets with stable identities. The group supports
millimeters or inches, 0–8 decimals, reference parentheses, prefix/suffix text, and
the same symmetric, deviation, limits, and basic tolerances as ordinary dimensions.
Individual target tolerances can override the group tolerance. Datum zero text
does not inherit dimensional tolerances.

The group's ordinary paper move handle changes its common offset. Each target
label has an independently draggable handle. Datum and target attachment handles
reattach to saved native edge points, circular centers, planar-face origins, or
sketch points. Current source/evaluation identity checks, capture cancellation,
live preview, Apply, Cancel, and undo use the existing drawing transaction.
No geometry is inferred from label positions or display samples.

Missing or ambiguous native attachments remain diagnostic failures. The repair
preview displays the group diagnostic and its attachment handles; final Apply and
export require current validation. Baseline/chain zero-length intervals and
nonfinite projections are rejected. Ordinate zero coordinates are allowed.
Isometric, detail, and section views need separate coordinate intent and are not
accepted by this group contract.

The authoritative contract is `tcad.drawing` version 8 with
`drawing_dimension_groups` version 1. Paper interaction contract version 3 adds
stable group-target attachment slots and label movement actions. Group definitions
are a boxed `AnnotationKind::DimensionGroup`; documents without groups retain
their existing source representation. Sheet duplication refreshes group target
identities, and Make Unique retargets their native point references.

The model-independent f64 paper vector renderer is shared by desktop preview,
PDF, and drawing DXF. Editor controls use the existing semantic drawing field
adapter and stable source addresses, so user layouts and CSS-like themes can
reposition and restyle them. Automatic collision avoidance, jogged ordinate
leaders, multiple coordinate origins in one group, and nonorthographic groups
remain future work.

Validation for this implementation is compilation, formatting, and strict
Clippy. Interactive drawing acceptance is left to the user; no tests or UI smoke
runs were performed.
