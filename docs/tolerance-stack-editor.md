# Linear tolerance-stack editor

Open **Tolerance stacks** under Inspection. Each saved analysis retains a result
requirements context and 1–128 ordered dimensional terms. A document supports
128 analyses. Add, duplicate, rename, edit or delete analyses within the shared
document draft; Apply commits one undoable operation and Cancel restores the
opening checkpoint.

Select a row to edit its name, inclusion, signed coefficient, definition and
configuration. Nominal lengths may be unit-aware expressions, linked named
length parameters, or linked linear driving sketch dimensions. Reference/driven
dimensions and angular dimensions cannot be used as linear nominal bindings.
Named configurations are explicit; **Active configuration** follows the
definition's currently selected configuration. Requirements and new term
defaults use the analysis context; existing terms retain their own contexts.

Lower and upper deviations are signed length expressions relative to nominal.
For example, a gap may be authored as `+1 × housing -1 × shaft`; each term has
its own deviation bounds. Lower and upper result requirements are optional
length expressions. The report shows current nominal, worst-case interval,
inclusive requirement status, per-term values, and diagnostics in millimeters.
Exceeded requirements remain a valid analysis result.

Variation assumptions are explicit:

- **Unspecified** supports worst-case arithmetic without inferring a process
  standard deviation.
- **Uniform** declares a uniform distribution within the saved deviation band.
- **Normal** requires a supplied nonnegative sigma and process mean offset;
  they are not inferred from engineering tolerance limits.
- A shared variation-group name identifies one repeated variation variable.
  All rows in a shared group must have identical evaluated deviation and
  process assumptions. Signed coefficients aggregate before interval and
  variance calculations, including cancelling coefficients.

Statistical results also require an explicit independence declaration for
different variation groups. The editable k-sigma multiplier lies between zero
and ten. Mean, sigma and k-sigma spread are conditional on those assumptions;
the spread is neither a worst-case guarantee nor an inferred process yield.

A persistent arithmetic worker retains one pending source snapshot and one
coalesced reply. It uses authored f64 expressions and configurations and does
not invoke geometry generation. Source generation, revision, document and draft
identity reject stale replies. Editing stays responsive while calculations are
pending. This editor's Apply requires its selected analysis to have a current
available reading. Missing bindings, incorrect units, reversed limits or
inconsistent shared groups remain visible repair diagnostics. Existing orphan
analyses do not disable unrelated modeling operations.

Raw invalid field/source text survives until corrected or cancelled. Stable term
identities govern field setters and row actions across reorder. Each source,
term, result, control and action publishes semantic data separately from the
native compact table, so user layouts and CSS-like styling can replace the
presentation. Advanced Source holds complete JSON; the normal editor exposes
task fields. **Export report** writes the current source-qualified JSON report
atomically, including unavailable readings and their diagnostics.

This contract covers linear source-dimensional stacks. Geometric measurements,
angular or nonlinear stacks, covariance matrices, inferred distributions, process
capability/yield certification and full 3D tolerance simulation remain separate
work. Compilation, formatting and linting are the implementation checks; no
tests or UI smoke runs are performed.
