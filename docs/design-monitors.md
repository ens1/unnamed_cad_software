# Saved design monitors

Design Monitors compare saved limits with current parameter values or evaluated
part and assembly properties. They provide design alerts while the model changes,
without changing geometry, component placements, or mechanical constraints.

Open **Design Monitors** from command search. Add a monitor, choose its target and
quantity, then set a lower limit, an upper limit, or both. Limits include their
endpoints. The editor supports enabling, renaming, deleting, notes, and Advanced
Source. Edits share the document draft: Apply waits for current model validation
and commits one undoable change; Cancel restores the starting document. Invalid
field text remains available for correction. A missing or suppressed reference
reports Unavailable and retains its saved binding for repair.

| Target | Quantities and authoritative units |
| --- | --- |
| Named part or assembly parameter | Scalar, length in mm, angle in rad |
| Part definition and selected variant | Mass in kg, enclosed volume in mm³, area in mm², local bounding sizes in mm |
| Complete assembly or full occurrence path | Mass, enclosed volume, area, and bounding sizes in model assembly coordinates |

Part and parameter targets can follow the active configuration or retain a named
configuration. A part variant needs current evaluated properties before a
geometric reading is available. Occurrence paths include all parents, so repeated
subassemblies can be monitored independently. Assembly readings include deposited
weld metal and reject partial totals when component properties are missing.
Hidden, ghosted, and exploded display arrangements do not alter mechanical readings.

Parameters use native expressions and configuration inheritance. Geometric
readings describe the evaluated boundary tessellation; they are not exact B-rep
integrals or certified error bounds. Mass uses assigned engineering densities.
Surface-only geometry has no enclosed volume or mass reading. Unknown values do
not become zero or pass a limit. Limits use canonical numeric units.

Calculations run on a persistent background worker. Its queue retains the newest
pending source snapshot, and the presentation rejects results from earlier
document, draft, or evaluation identities. Property snapshots omit viewport
meshes, configured parameter contexts have bounded scratch caching, and idle
frames reuse completed readings. Pending calculations show Unavailable rather
than retaining an earlier passing result.

`cad_core::sensors` owns version 1 definitions and reports. The document stores a
`sensors` map, omitted when empty, with up to 512 monitors. Limits and readings use
`f64`. Semantic `design-monitor`, `design-monitor-field`, and
`design-monitor-action` elements expose source, results, controls, and states
independently of their native panel layout.

**Export readings** writes JSON after the current calculation completes. The CLI
provides `sensors PROJECT --output REPORT.json`; `--source-only` evaluates
parameter monitors and reports geometric readings as unavailable. Optional CLI
tolerance overrides affect rebuilt readings. Associated distance and angle
monitors, automatic constraint rules, tolerance stacks, and certified geometric
integration remain separate work.
