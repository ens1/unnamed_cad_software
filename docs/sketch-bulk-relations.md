# Multiple-entity sketch relations

Cmd-click geometry in the viewport to add or remove it from the sketch
selection. Ctrl-click is also supported, and Shift-click retains selections in
Select. Modified geometry picks switch from drawing to selection without placing
a new entity. They leave the floating menu closed so subsequent picks remain
accessible. Right-click one selected entity to open the selection's menu without
discarding its other members, or choose a relation in the property manager.

Relations validate the complete selection before changing source. A compatible
batch creates one sketch-history checkpoint, updates the pending preview, and
remains subject to **Finish Sketch** / **Finish 3D Sketch** validation and the
tool's Cancel checkpoint. An incompatible entity rejects the complete batch;
the implementation does not silently constrain a subset.

## Planar sketches

| Relation | Multiple-entity behavior |
|---|---|
| Horizontal / Vertical | Orient every selected straight member, or align the Y / X coordinates of selected points or point-like centers. |
| Equal Length | Give all selected straight members the first selected member's length. |
| Parallel / Perpendicular | Make every remaining selected straight member parallel / perpendicular to the first. A universe axis can supply the direction reference. |
| Equal Radius | Join selected circle/arc size groups using their existing references. |
| Concentric | Match each selected circle/arc's center to the first selected member. |
| Coincident | Join selected points, or put multiple points on one selected line, axis or supported curve. Existing two-entity center relations retain their behavior. |
| Midpoint / Arc Center / Point on Bounded Arc | Put every selected point at the indicated location on one selected carrier. |
| Origin / On X Axis / On Y Axis | Anchor every compatible selected point or member. Straight members on an axis also adopt its direction. |
| Fix | Fix every distinct selected point or member control at its current position. |

The remaining lines in a Perpendicular batch are each perpendicular to its
reference; the command does not demand pairwise perpendicularity between every
line. Universe axes have direction rather than physical length, so Equal Length
requires actual sketch members.

The first selected entity supplies the reference for ordinary size/direction
groups. Existing circular equalities keep their original size ancestry even if
a follower is selected first. Repeated, reversed or transitive circular equality
selection is a no-op. New groups join their existing reference members. An
existing driving Radius/Diameter still takes precedence; conflicting driving
dimensions can leave the pending sketch unsatisfied. Ambiguous legacy cyclic or
competing undimensioned references require repair before merging groups.

## Spatial sketches

Along X/Y/Z applies to every selected line. Equal Length supports selected lines
and three-point arcs; Equal Radius supports full circles. Parallel and
Perpendicular use the first selected line as reference. Coincident supports
multiple points or multiple points and one carrier. Point On Line, Point On
Curve and On Workplane apply to every selected point with the appropriate single
carrier/current workplane. Fix XYZ covers distinct controls throughout the
selection.

Equal circles share one effective associative radius expression. One existing
radius/circumference dimension can govern the group. Follower radius fields show
their inherited expression and identify the reference instead of accepting
dormant seed edits. Extra driving size dimensions or invalid reference graphs
are diagnosed explicitly. Already-connected groups retain their existing owner.
See [the spatial solver contract](sketch3d-constraints.md).

Tangency/curvature and symmetry retain explicit pair/triple selections where
contact or reflection would otherwise be ambiguous. Smart Dimension retains its
deliberate one/two-reference workflow; selecting a larger group does not create
an inferred dimension family.

## Tangent selection

Planar **Tangent** stays visible in the property manager and sketch selection
menu. It is disabled with an actionable reason until exactly two compatible
references are selected. Cmd-click the two curves, or use their endpoints or
circle/ellipse centers when curve ownership is unambiguous.

A selected endpoint fixes the native tangent contact to that end of its curve;
the other curve can retain a free bounded contact parameter. Center picks select
their owning circular/elliptical curve without making the center a contact point.
Shared points with several possible curve pairs, standalone points, arc
through-points and interior spline controls require the actual curves to be
selected. Picking two references owned only by one curve also receives a clear
diagnostic.

Whole-member generic tangency permits free contact locations. An unambiguous
authored endpoint join fixes contact there; coordinate proximity alone does not
create a join. Construction curves participate normally. Universe-axis tangency
remains limited to supporting circle/arc carriers; fixed endpoint contact needs
two actual sketch curves, so a construction line supplies an explicit axis-like
carrier. Internal Tangent and Curvature Continuous retain two direct member
selections. The existing native tangent equations and validation remain in use.

## Presentation and validation

The existing planar `sketch.relation.*` and spatial `sketch3d.action` commands
remain renderer-independent. Relation descriptors publish selection count,
availability and selection-semantics descriptions; spatial actions also publish
disabled reasons. Native property controls and context menus use those commands.
Themes retain control of their layout and styling.

This addition uses native source definitions and solver constraints, without a
persisted batch or renderer object in the document. Current backend identity is
adapter 42 / solver 18. Compilation, formatting, strict linting and signed macOS
packaging establish integration; interactive acceptance remains with the user.
No tests, probes or app launches were performed.
