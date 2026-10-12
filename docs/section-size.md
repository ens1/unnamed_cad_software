# Evaluated section sizes

`SectionSize` version 1 is optional derived manufacturing/display metadata. It
records analytic section extents, an outer diameter when established, and an
optional constant wall. Adding `wall_mm` is compatible with existing version 1
records; missing wall data remains absent. Section IDs, material identity, stock
equivalence, cut-list grouping, and source definitions are unchanged.

Sizes come from the current validated solved sketch's analytic points and circle
radii in its orthonormal section frame. Display polylines, body tessellation,
nominal catalog text, and unsolved source radius expressions do not establish
these measurements. Construction and suppressed members are excluded, including
legacy circle records mirrored by modern construction members.

One circular outer material boundary establishes its outside diameter. A single
circular hole with exactly the same evaluated center and a smaller positive
radius establishes an annular wall. A rectangular tube requires one outer and
one inner four-line material contour: each has four distinct, complete,
axis-aligned sides. Its hole must be strictly inside the outer boundary, with the
same positive setback on all four sides. Error-free two-part coordinate
differences compare the actual stored dyadic setbacks, so merely rounded equal
subtractions do not manufacture a uniform wall.

Wall classification uses evaluated carrier coordinates without proximity-based
point merging. The section frame's unit/orthogonality check is bounded to floating
arithmetic roundoff. A circular tube displays `ØD × wall`; a proven rectangular
tube displays `W × H × wall`. Other supported noncircular profiles retain an
explicit `extents` label. Offset holes, unequal setbacks, skewed or rounded
rectangles, ambiguous material loops, and unavailable analytic data do not acquire
a wall value. General curved extents remain outside this bounded classifier.

Values remain f64 millimeters; display converts to the requested drawing units
and precision. Small positive quantities use scientific notation when fixed
precision would display zero. Legacy material circles can supply their analytic
diameter, but incomplete legacy contour membership does not prove a tube wall.

Engineering checks are compilation, formatting, and linting. No code tests,
geometry probes, or interactive acceptance checks were performed.
