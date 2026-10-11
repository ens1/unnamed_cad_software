# Native sketch-driven ribs

`tcad.rib` version 1 saves the open sketch profile and native member anchors,
supporting body outputs, thickness placement, material direction, termination
and authored attachment overlap. The source contains no kernel handles or
rendered geometry. Rib is a solid feature created from an open sketch; it does
not expose a primitive creation command.

The familiar Rib feature groups direction, first/both/second-side thickness,
selected contours and body attachment. These reference controls are described
by the [SolidWorks Rib PropertyManager](https://help.solidworks.com/2016/english/solidworks/sldworks/hidd_feat_rib.htm)
and [Rib sketch examples](https://help.solidworks.com/2021/english/SolidWorks/sldworks/c_Example_Rib_Feature_Sketches.htm).
TCAD labels its direction by the actual material-fill axis: Parallel to Sketch
fills within the sketch plane; Normal to Sketch fills along its normal.

## Authoring and source identity

Create and constrain an open contour in the same model viewport. Rib can use
all current open contours or selected connected chains. A selected native
member identifies its complete open chain; contained closed contours are not
rib centerlines. Missing, construction, suppressed and newly closed member
selections fail explicitly. Connectivity follows source endpoint identities
and canonical constraint aliases. It does not connect paths by displayed mesh
vertices or invent an attachment from a tessellation.

The docked manager exposes thickness, thickness placement, reversal, material
direction, bounded body/face termination, attachment overlap and additional
feature scope. Parallel-mode material direction uses scalar expressions along
the current sketch U/V basis. Changing an associative sketch plane updates
these directions. Lengths remain authoritative f64 expressions with explicit
units. Normal mode uses the current evaluated sketch normal.

Commands invoked in an open sketch preserve a document/part-bound pending Rib
intent. Finish Sketch captures the final selected member IDs before clearing
sketch state, then opens the feature manager after successful sketch Apply.
Cancel clears the pending intent. A pending action cannot cross into another
document or part. Existing feature/configuration/pattern editors guard entry.
Rib creation and edits use the shared draft, persistent worker, current-result
validation and checkpoint. OK commits one undoable operation; Cancel restores
the original definitions and output branches.

## Current exact geometry

Parallel to Sketch closes each open path by projecting its endpoints toward a
bounded planar support, retaining the actual native path as the upper boundary.
It creates a filled planar section and thickens that section normal to the
sketch, with first-side, centered or second-side placement. Line chains, arcs,
Bezier curves and positive-weight rational splines are accepted when every
retained span proves monotone progress across the fill axis. This proof prevents
folds and uncertain self-intersection. The support must contain the thickness
direction. Vertical-return segments and curves with uncertain control hulls
are rejected rather than flattened into a polygon.

Normal to Sketch constructs an exact in-plane thin strip from the selected
open line/arc chains, then projects matching caps along the sketch normal to
the bounded planar support. Oblique planar terminations retain their actual
cap orientation. The initial strip must lie wholly outside and forward of its
selected attachment face. General spline offsets remain unsupported in this
mode.

Up to Body resolves the nearest complete forward supporting face for each
contour. Full distance envelopes must establish an unambiguous nearest face.
Up to Face stores the actual topology reference and solid output owner; it
must resolve uniquely in Feature Scope. Whole-footprint containment is proved
against the face's bounded convex straight polygon or supported exact circular
boundary, preserving disjoint holes. Native multi-arc circular caps use the
carrier's polynomial circular identity and complete Bernstein angular-derivative
coefficients to prove one full outer turn; display samples do not establish
coverage. Crossing envelopes, partial footprints,
curved supports and piecewise multi-face terminations require further Rib
intersection and trimming work.

Attachment Overlap is an authored positive length, default `0.1 mm`, along the
material-fill direction. It creates a real volumetric interface with the
support. The whole collar must be contained inside the supporting stock:
entry coverage plus supporting half-spaces or disjoint exact positive control
hulls establish that other exterior/cavity boundaries cannot cross the collar.
An oversized collar or uncertain containment fails; the evaluator never adds
hidden penetration or reports a touching/disconnected body as an attached rib.
Convex planar stock and stock with provably disjoint existing voids are the
principal domain. Rational curved neighboring walls use positive control bounds
and a proven separating projection when available. Other nearby concave/curved
boundaries can prevent the proof.

The rib solids are validated B-reps and unioned with all current bodies in the
selected output scope. Native member and closure ancestry names the generated
cap/wall topology, with the actual supporting face in construction intent.
Generic Boolean splitting uses the adapter's construction history. Coincident,
tangent or otherwise inconsistent Boolean boundaries remain explicit failures;
successful compilation does not establish broad kernel robustness.

## Remaining engineering

Natural extension of open curves to side walls, partial/multi-face and curved
support trimming, arbitrary concave footprints, non-monotone centerlines,
drafted ribs, broader thin spline offsets and exact intersecting-rib healing
remain work. The current feature is an end-to-end native Rib implementation
within the stated geometry domain, not a claim of full reference-product parity.

No code tests, assertion probes, manual smoke checks or interactive acceptance
were written or run. Parent integration owns compilation, formatting and lint;
interactive acceptance remains with the user.
