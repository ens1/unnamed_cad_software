# Fabrication output conventions

Flat-pattern DXF uses millimeter coordinates and AC1027 encoding. Each selected
sheet receives readable `CUT_01`, `BEND_UP_01`, `BEND_DOWN_01` and
`BEND_NOTES_01` layers, followed by the next sheet number. Sheet identity is
preserved separately in registered `TCAD` extended entity data, together with
the source feature and thickness. Layer numbering is presentation, not identity.

Every nonempty knot span of a degree-one flat contour exports as an exact LINE.
Higher-degree curves retain their rational SPLINE representation. No tessellated
substitute is introduced. Bend lines carry source member/index, signed angle in
degrees, inside radius, bend allowance and K factor in extended data. UP/DOWN
uses the signed bend relative to the sheet's authored base frame. Bend notes on
their own layer show direction, angle and inside radius; users of cutting tools
can omit both bend and note layers. External CAD/CAM import acceptance remains
separate from compilation and source review.

Cut-list CSV headings include the chosen length units and degrees for angles.
When Length is exported, a Length basis column follows it, including for saved
column choices that predate this column. Straight-member values measure the
reference path between resolved end planes. Curved-member values measure the
retained reference-path arc. Accessory values use the maximum blank dimension.
These values are not claimed to be longest/shortest saw-cut dimensions. Nominal
section, engineering material and optical finish retain their separate columns.

These changes implement release-review recommendations RR-12 and RR-19. No
code tests, assertion probes or interactive acceptance runs were performed for
this implementation. Formatting, compilation and lint results are recorded in
the implementation commit and release follow-up report.
