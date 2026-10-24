# TCAD implementation

Build the complete application end to end. The user's latest instruction supersedes milestone-first, slice, spike, probe, and test workflows in the handoff package. Do not write or run code tests or manual smoke tests. Compile, format, and lint code; leave interactive acceptance to the user.

The product baseline is the mainstream mechanical CAD capabilities shared by SolidWorks, NX, Onshape, and Creo. The user's 2026-10-10 clarification replaces full SolidWorks parity; equivalent practical workflows remain required. Cover sketching, solid and surface modeling, reference geometry, assemblies and mates, production drawings and tolerancing, sheet metal, structural design, mold/tooling workflows, configurations, libraries, inspection, and CAD exchange. See docs/mechanical-cad-coverage.md for the dated references and current gaps. Count actual supported operations, modes, geometry, associativity, and editing behavior; command names and partial implementations do not establish completion. Vendor-specific niche features are not required merely because one suite offers them. Missing kernel or solver capabilities needed for the shared workflows are engineering work within scope. Simulation, routing, CAM, electrical design, and enterprise PDM remain outside this clarified baseline; TCAD's existing local-file and Git requirements remain in scope.

Read mechanical-cad-build-package/docs/01-product-spec.md and docs/11-replaceability-and-presentation.md under that package. Keep authoritative definitions independent of renderer/kernel objects. Use replaceable application-owned contracts and versioned adapters. Rust, Bevy, and Monstertruck are initial implementations.

Expose semantic UI information/actions separately from user-defined layout and CSS-like styling. The instrument rail is the default. All editors share the document, draft, commands, and undo history. Apply requires the current validated evaluation; Cancel leaves committed definitions unchanged. Geometry runs in persistent worker processes, with stale-result rejection and responsive navigation.

The user's CAD-convention feedback supersedes the original rail-only chrome: default to an empty Part1, a menu/command bar with Features and Sketch tabs, a left feature tree/property manager, named Front/Top/Right planes and Origin, and one model viewport. Keep Flow optional and demo documents explicitly opt-in. Use task-specific fields and familiar feature names; keep IDs and complete JSON under Advanced Source. Modeling OK waits for current validation and commits one undoable operation; Cancel restores its starting checkpoint. Do not substitute a demo model for a new document.

New modeling starts from sketches, without primitive creation commands. Sketch drawing and editing happen on an origin plane or planar model face in the same model viewport as the part. Tool controls remain docked in the main workspace. Orbit must allow complete rotations, including across camera poles. Keep layout and editing-state sizing user-defined.

Keep f64 authority, explicit units/tolerances, stable IDs, construction/provenance-based references, local files, and recoverable saves. Report unsupported cases clearly rather than substituting fake geometry. Original code uses WTFPL; retain dependency licenses.

Bundle executables must have filenames distinct on case-insensitive filesystems: `TCAD`, `tcad-worker`, and `tcad-cli`. Never package the CLI as `tcad` beside `TCAD`; it overwrites the desktop executable on macOS.

## Repository workflow

The canonical repository is `https://github.com/ens1/unnamed_cad_software.git`.
The user made TCAD public on 2026-10-24. Commit cohesive implementation changes
regularly, including substantial completed checkpoints during longer tasks,
and push completed commits to this origin. Keep newly created user model
repositories private by default. Use descriptive messages and record the
compilation, formatting and linting actually performed; do not introduce or run
tests. Preserve unrelated
user changes and never rewrite published history without explicit authorization.

The current Rust workspace is one public monorepo. Additional repositories and
public upstream dependency forks are authorized when engineering needs justify
them. Retain upstream licenses and pin dependency revisions. Public forks must
contain the relevant dependency work without exposing private design data.
Do not create extra repositories merely to separate existing replaceable crate
boundaries.
