# Sheet metal, structural stock, and deposited weld metal

Sheet stock and structural groups use version 2 application-owned contracts. Weld beads use version 1. Native geometry and display meshes remain derived caches; construction references retain the feature, source member, and operation role. The shared modeling draft evaluates changes before OK, accepts only the current validated source, and restores the checkpoint on Cancel.

## Sheet metal

Base flanges retain closed material regions, holes, thickness direction, inside radius and K factor. Edge Flange can attach to an unused complete supporting base edge or a prior bend's terminal edge. The member identity follows the chosen parent attachment. Signed angles extend to 180 degrees. Formed chain geometry uses native analytic circular bends and sewn tangent interfaces.

Hem creates an open return with the specified physical clear gap; its inside radius is half the gap. Jog uses two opposite bends, signed finished mid-surface offset, bend angle, and terminal tangent length. Web length is derived from the requested offset and actual bend radius/thickness. Flat Pattern retains both jog bend lines and developed neutral-axis allowance. Geometry with insufficient offset or proven interference is rejected.

Normal Cut accepts a closed sketch on a plane parallel to the original blank, including a planar stock face. The native profile is projected onto the thickness interval and applied to both formed and developed stock. Tabs and cuts retain ordered blank ancestry. Cuts changing a flange's root interface, cuts across angled formed flanges, partial-width attachments, crushed hems, general corner relief/rips, swept/miter flanges and arbitrary solid recognition/unfolding remain unsupported.

## Weldments

Structural member groups accept authored planar lines, circular arcs and circles, including construction paths. Curved stock uses native revolution and tangent section frames. End offsets and planar trims determine actual developed centerline lengths. Joint Gap and Trim to Primary Member apply to supported straight stock joints; member ordering defines priority. End caps offer filled outer or hollow inset contours, edge allowance, terminal gap and bounded recessed placement. Curved member ends use their tangent frames.

Cut lists and drawing cut-list tables include curved radius/sweep and associated cap/gusset rows. Accessories retain feature/stock identities, thickness, blank size and whether those bounds are exact or conservative. General spatial/spline members, branch miters, arbitrary body/tool trimming, profile standards libraries and general longest-blank optimization remain gaps.

## Weld beads

Weld Bead deposits actual native solids along an associative model edge, sketch member or reference curve. Fillet uses equal legs with a circular crown; Butt uses a circular cap with an editable width. Continuous or Intermittent deposition retains physical bead and gap lengths, start/end offsets, and stable segment identities. Authored sketch/reference seams provide direction and roll. Optional engineering material and optical finish assignments are independent.

Part beads create independent body outputs without consuming or fusing their parent stock. Assembly beads are saved in their owning assembly. Component paths resolve in the selected configuration and mate-solved positions; support faces can come from different components. They do not create mates. Moving the components away from a proved joint invalidates that bead rather than fabricating a bridge. Convex exterior edges are not fillet joints: select the two actual joint support faces when automatic adjacent-face inference is unsuitable.

Supported paths are connected, non-retracing collinear lines or coaxial coplanar circular arcs. Supported fillet contacts use two planes, or a plane and a bounded cylindrical patch. Arbitrary seams, general multi-face transitions/corners, unrestricted cylindrical trim patches and complete groove profiles remain unsupported. Assembly welds are omitted from exploded presentations because those presentations separate their joint supports; assembled model views, drawings and STEP/STL exports include deposited metal.

Bead metadata reports analytic deposited length, section area, segment count and volume. The engineering property panel separately integrates the evaluated boundary tessellation for centroid, area, mass and inertia; it does not claim an exact B-rep property certificate. Manufacturing bodies and beads remain separate, so stock touching or intentionally overlapping a bead is not an automatic fusion.

## Verification

Compile, format and lint only. No code tests or manual modeling smoke checks are written or run; interactive acceptance belongs to the user.
