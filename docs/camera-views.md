# Named camera views

TCAD stores named camera views as document source, separately from geometry,
configurations, display states and exploded arrangements. View → Named views
opens the compact manager. **Save current view…** captures the current camera;
Edit exposes its name, target, yaw/pitch/roll, distance and projection. **Update
from current view** captures a new camera for the selected saved definition.
Delete is staged in the shared draft. Apply commits one undoable source change
after current validation; Cancel restores both the source checkpoint and the
camera from the start of that editor. Opening and closing an unchanged editor
adds no undo entry.

**Recall changes only navigation.** It does not dispatch a document command,
start a geometry evaluation, move components, switch configurations, change
hidden/ghost finishes, inspect another feature-history stage or alter exploded
view selection. The saved definition contains no part, face or occurrence
references. Missing/deleted bookmarks have no guessed replacement. A recalled
view remains freely navigable; orbiting, panning or zooming does not rewrite the
saved bookmark. The current marker compares normalized pose values rather than
assuming a previously selected bookmark is still the current camera.

Perspective and Orthographic are directly available from the View menu and
semantic command controls, before saving a bookmark. Switching projection
preserves target-plane framing within the documented distance/span bounds.
Perspective stores a vertical field of view; orthographic stores a vertical
visible span in millimetres. Perspective rays originate at the camera eye;
orthographic rays use shifted origins and parallel directions. Model selection,
hover guides, sketch geometry, dimensions and handles share the same projection
and image aspect. Duplicated viewport regions use the actual clamped render
texture aspect, including when a large display exceeds the texture dimensions.

Pinch/scroll zoom preserves the cursor anchor in both projections. Orthographic
zoom changes the visible span; perspective zoom changes camera distance. Pan,
unlimited orbit across poles, native roll and navigation actions use the common
f64 pose. Pan sensitivity scales with the target-plane visible span in either
projection, including a named perspective view's field of view. Fit updates the
active projection scale using its field of view and
viewport aspect without overwriting saved views. Older fit, sketch-normal and
focus callers remain supported through detected legacy camera-mirror changes;
an orthographic camera interprets their distance change as a scale change.

## App-owned source and semantic controls

`tcad.camera-views` version 1 is defined in `cad-core::camera_views`:

- `Document.camera_views` is a stable-ID map of `Definition { version, id, name,
  pose }`. Names must be nonempty, at most 128 bytes and unique ignoring case.
  Up to 256 definitions are supported. An empty map is omitted, preserving older
  document serialization and source hashes.
- `Pose` owns f64 native-CAD `target_mm`, `orbit_rad`, `roll_rad`, `distance_mm`
  and `Projection`. Perspective carries `vertical_fov_deg` (5–120°).
  Orthographic carries `vertical_span_mm` (0.1–100,000,000 mm). Distance accepts
  0.5–1,000,000 mm; targets accept finite coordinates within ±1,000,000,000 mm.
  Angular source values are finite and bounded; navigation normalizes them with
  full rotations and preserves pole crossings.
- `PresentationState.camera_pose` is active f64 presentation intent.
  `camera_projection` defaults to the unchanged 45° perspective. Existing
  transient f32 orbit/target/distance fields remain adapter mirrors, so older
  direct camera callers continue to work. A mismatch invalidates the old mirror
  binding. Legacy captures retain the precision that legacy values actually
  supplied; they do not claim additional measured precision.
- Camera frame, projection/ray calculations and navigation use f64 coordinates.
  Conversion to f32 occurs at the Bevy/rendered-control boundary. Display meshes
  and GPU transforms still have the renderer's f32 precision; the bookmark does
  not imply arbitrary-scale rendering or change CAD geometry precision.
- `PutCameraView` and `DeleteCameraView` use the existing command/draft/history
  contract. Native local saves, recovery and document versions carry these
  definitions. Component library linking does not replace destination scene
  bookmarks with a component's scene settings.

Semantic roles include `camera-view-library`, `camera-view`, `camera-view-field`,
`camera-view-source`, `camera-view-action` and `camera-projection-action`.
`workspace.camera-views` exposes the `camera-views-editor` renderer. Commands are
`camera-views` and `camera-view.save`, `.edit`, `.rename`, `.update`, `.delete`,
`.recall`, `.perspective` and `.orthographic`. Independent field bindings use the
`camera-view` setter; native typed fields and complete Advanced Source use the
same source, target identity, editor generation and invalid-value retention.
UI angles are displayed in degrees while the pose retains radians. Invalid
numbers, malformed source and stale targets cannot silently apply or poison a
new editor generation. All labels, fields, actions, source records and active
pose information are available to user layouts and CSS-like styles; the compact
native manager and scrollable named-view menu are defaults.

The source contract has no renderer handles, camera entities or kernel objects.
Saved-camera recall intentionally leaves drawing view orientation and drawing
sheet definitions unchanged; drawing associative named model-view binding is a
separate remaining workflow. Implementation validation consists of compilation,
formatting and linting. No automated tests, probes or interactive acceptance runs
were written or performed.
