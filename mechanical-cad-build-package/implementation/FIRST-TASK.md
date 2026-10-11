# Complete application implementation task

Use this prompt with the repository's current instructions. The former M0 probe prompt is superseded by the user's complete-application direction and mechanical CAD baseline.

~~~text
Build the complete TCAD application end to end. SolidWorks mechanical CAD
is the minimum feature baseline, including practical feature modes,
supported geometry, associativity, editing behavior, and manufacturing
outputs. Do not narrow delivery to milestones, slices, spikes, or probes.

Read the repository's root AGENTS.md, docs/mechanical-cad-coverage.md,
mechanical-cad-build-package/docs/01-product-spec.md, and
mechanical-cad-build-package/docs/11-replaceability-and-presentation.md.
Inspect existing implementation and preserve the user's work. The coverage
inventory records gaps, not an exhaustive count or a parity claim.

Cover sketches and constraints/Smart Dimension, solids, surfaces,
references, patterns, multibody/direct editing, assemblies and mates,
production drawings/GD&T, sheet metal, weldments, molds, configurations,
equations, libraries, inspection, and CAD exchange. Simulation, routing,
CAM, electrical design, and PDM product-family parity are outside this
clarified baseline. Retain TCAD's local-file and Git requirements.

Build like a ship of Theseus: every subsystem, including core, kernel,
solvers, renderer, widgets, transport, and persistence, must be replaceable
through application-owned versioned contracts. Rust, Bevy, and Monstertruck
are initial implementations. Vendor objects stay inside adapters; the
document uses stable application IDs, f64, and explicit units/tolerances.
Keep analytic/NURBS B-rep authority and construction/provenance references.

Expose semantic UI information and actions independently of widgets,
layout, and CSS-like styling. Users control composition, grouping, size,
position, and appearance. New features publish semantic commands/data.
All editors share document, draft, commands, and undo history.

Default to empty Part1, a menu/command bar with Features and Sketch tabs,
a left feature tree/property manager, named Front/Top/Right planes and
Origin, and one model viewport. Flow is optional, with the orange
instrument rail as its visual reference. Demos are explicitly opt-in.
Start new modeling from sketches without primitive creation commands.
Sketch on origin planes or planar model faces in the model viewport.

Group related tools sensibly. Use familiar feature names and task-specific
fields; keep IDs/full JSON under Advanced Source. Clicking actual model or
sketch geometry reveals applicable actions; ordinary UI/background clicks
do not open a geometry popup. Sketch handles drag with constraints and
live previews. Every sketch member may be construction geometry.

Geometry runs in persistent workers. Keep navigation responsive, allow
unrestricted orbit through camera poles, and reject stale evaluation
results. Real evaluated previews update before Apply. Modeling OK waits
for the current validated result and commits one undoable operation.
Cancel restores the starting checkpoint. History inspection displays
cached feature-stage geometry without rebuilding or changing definitions.

Fix or replace kernel/solver implementations to meet the required feature
set. Keep unsupported cases explicit, preserve valid source on failures,
and never substitute fake geometry. A narrow loft, sweep, shell, or sketch
operation remains partial; a toolbar label does not establish coverage.
Update the coverage inventory and implementation limits with actual
supported modes, source locations, versions, and available evidence.

Do not write or run code tests, assertion probes, or manual smoke tests.
Compile, format, and lint code; retain runtime schema/constraint/geometry
validation and user-facing headless commands. Leave interactive acceptance
to the user. Do not treat compilation as proof of geometry or usability.

Retain dependency licenses. The original application code uses WTFPL.
Bundle executable names must be distinct on case-insensitive filesystems:
TCAD, tcad-worker, and tcad-cli. Never bundle a CLI named tcad beside TCAD.

Deliver real integrated functionality and report supported behavior,
compilation/lint evidence, and remaining material limitations honestly.
~~~
