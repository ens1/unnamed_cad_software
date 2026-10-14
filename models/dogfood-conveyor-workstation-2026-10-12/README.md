# Conveyor handling line review models

These projects were authored through TCAD's desktop UI during the October 12–13, 2026 usability review.

- Open [conveyor-line.tcad/project.json](/Users/tyler/Software/Personal/tcad/models/dogfood-conveyor-workstation-2026-10-12/conveyor-line.tcad/project.json) for the three-module handling-line concept. It contains 102 active part occurrences, with modules patterned at 2150 mm pitch.
- Open [upper-frame.tcad/project.json](/Users/tyler/Software/Personal/tcad/models/dogfood-conveyor-workstation-2026-10-12/upper-frame.tcad/project.json) for the reusable module. Its older filename reflects the original frame part. Part2 is the leg, Part3 the roller sleeve, Part4 the formed guard, Part5 the bored mount and Part6 the shaft.
- [side-guard-flat.dxf](/Users/tyler/Software/Personal/tcad/models/dogfood-conveyor-workstation-2026-10-12/side-guard-flat.dxf) is the UI-exported guard flat pattern.
- [The usability report](/Users/tyler/Software/Personal/tcad/docs/conveyor-project-usability-review-2026-10-13.md) records successful operations, blockers and the recommended improvements, including direction picking for linear patterns.

The saved assembly hierarchy is preserved, although the current UI tree flattens its children. Pattern source parameters determine active occurrence positions; retained placement records alone do not show the final solved pattern layout.

This is an unfinished engineering concept. It includes twelve roller sleeves and twelve front mounts per module, but only one shaft, no rear mount row, no completed mate network, and no committed drawing sheets. Guard penetrations, bracing, feet, inserts, fasteners, weld beads, material assignments and transfer details remain incomplete. The counterbore and larger assembly drawing workflows encountered blockers documented in the report.
