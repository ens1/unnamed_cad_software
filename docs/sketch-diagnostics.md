# Sketch freedom and relation repair

The sketch manager's **Freedom** toggle displays local positional movement
directions directly on the sketch. A point can have no movement direction, one
direction, or two independent directions. Selected members include their actual
control points; selected relations include their participating geometry. With
no selection, all analyzed points are shown. Constrained positions use a small
marker. Generated points show motion inherited from their sources, retaining the
existing source-driven editing rules.

**Dependent** shows exact authored duplicate equations and local numerical
dependencies. Dependencies are computed in authored relation order. Direct size
expressions and other equations with no positional derivative are reported
separately; they are not automatically called redundant. Existing Unsatisfied
feedback continues to describe equations at the current authored geometry after
a failed solve. It does not invent directions from the previous valid geometry.

**Remove duplicates** compares kind, ordered entities, expression, driving mode,
curve contact and oriented distance side. It removes only an exact duplicate of
an earlier retained equation. IDs belonging to grouped dimensions are protected.
No expression normalization, geometric guessing or nonlinear relation removal is
performed. Command/Shift-click relation rows to select multiple dimensions;
**Selected → Reference** changes driving dimensions into measurements and retains
their original expressions for a later switch back to driving.

Each repair is one pending sketch history edit. The native solver and shared
document preview update immediately. Invalid results block Apply Sketch; Cancel
Sketch restores the operation checkpoint. No repair silently changes committed
definitions or weakens engineering constraints during a drag.

## Application-owned contract

`cad-core::sketch_diagnostics` contract v1 exposes stable point and constraint IDs,
f64 local XY directions, variable count, equation rank, local degrees of freedom,
actual maximum residual, generated-point provenance and per-equation dependency
counts. `ConstraintSolver::analyze_sketch` is an optional solver capability. Native
solving and analysis share the coordinate layout, contact/block variables, curve
dimensions, prepared constructions and residual blocks. The implementation
fingerprint is `tcad-native-lm/13`; changing implementations invalidates derived
caches without migrating source sketch definitions.

The numerical derivative and column-scaled SVD are evaluated only at a current
geometry whose maximum scaled residual is at most `1e-6` and which passes the
native sketch geometry checks. Disagreeing SVD and ordered row-space rank also
withhold motion feedback with an explicit numerical uncertainty reason. Directions are local
infinitesimal positional freedom. They do not certify finite motion, singular
geometry, global constraint independence or a minimal conflicting set. Point
rank uses the mapped numerical nullspace; generated constructions have finite
derivative guards. Directly authored curve sizes remain parameter expressions,
not additional freely solved point coordinates.

The analysis limits are 512 free coordinates, 2048 scalar equations and two
million point derivative entries. Reaching a limit or uncertain/nonfinite
derivatives reports an unavailable reason and leaves normal editing and native
solver validation enabled. The desktop sends the Jacobian/SVD and derived-point
work to a separate persistent worker after a 90 ms debounce. A temporary request
sequence, document generation, base revision, draft edit, backend, owner/sketch
IDs and exact editor source all gate result acceptance. New edits terminate
outdated in-flight work; idle processes remain available across edits. Failed
or unavailable analysis has a retry control and never blocks modeling Apply.
Immediate authored-equation residuals and exact duplicate feedback remain local.
Source equality checks prevent old analysis from appearing after local edits,
failed solves, Cancel or document replacement.

## Presentation contract

The sketch feedback descriptor publishes `motion_analysis`, `motion_error`,
`show_motion`, `exact_duplicates` and each constraint's
`local_equation_diagnostic`. Point descriptors use stable
`<sketch-id>::motion.<point-id>` identities and the `sketch-point-motion` role,
with `constrained`, `one-direction` or `two-directions` states and `source` or
`generated` classes. Color, size, stroke width and display/visibility are theme
properties; the f64 direction data contains no pixels or theme values.

The `sketch-motion-toggle`, `sketch-motion-note`, `sketch-repair-action` and
`sketch-feedback-filter` controls publish ordinary semantic commands. The
`show-motion` field uses the registered `sketch-feedback` setter. Duplicate
custom controls share view state, authoritative selection and the same pending
sketch transaction. Point-motion controls can select their source point.

Compilation, formatting and linting are the available implementation checks.
No automated or manual smoke tests were introduced or run; interactive
acceptance remains with the user.
