# Linear tolerance stacks

**Inspect → Tolerance Stacks** opens a saved dimensional-stack editor. A stack
contains ordered terms with stable IDs. Each term has a signed coefficient,
nominal source, lower/upper deviation, and optional process distribution.
Native length parameters and driving linear sketch dimensions retain their
definition, configuration, sketch and constraint identities. Literal terms and
deviations accept explicit length units and named parameter expressions.

Scope defaults to the definition's active configuration. An explicit
configuration stays attached to that identity. Missing, suppressed or invalid
bindings produce an unavailable reading; they never become zero or silently
switch to a different dimension. Driving values are authored nominal intent,
not a measured separation between evaluated faces. Reference dimensions and
angular dimensions are not linear nominal sources.

Choose a term to edit its fields, add/copy/remove terms, or change their order.
Disable a term to omit it without deleting its source. Result requirements may
have a lower limit, an upper limit, or both. The report lists each nominal
contribution and the inclusive worst-case result interval. A calculation with no
requirements reports **Calculated**, rather than implying acceptance.

## Variation assumptions

The nominal result is the sum of coefficient times nominal. Worst-case lower
and upper deviations use the coefficient's sign when ordering each term's
endpoints. Independent physical variations remain distinct by default, even
when two terms refer to the same nominal expression. If terms share one physical
variation, give them the same **Variation group**. Their coefficients combine
before calculating that group's bounds and spread. Group members must have the
same deviation and process assumptions; inconsistent declarations are rejected.

Statistical results require an explicit declaration that the resulting groups
are independent, and a distribution for every group with a nonzero combined
coefficient. **Unspecified** is the default. **Uniform** uses the declared
deviation interval, whose mean is its midpoint and whose standard deviation is
its width divided by √12, as described by the [NIST uniform-distribution
reference](https://www.itl.nist.gov/div898/handbook/eda/section3/eda3662.htm).
**Normal** requires a supplied process sigma and mean offset; a dimensional
tolerance does not imply either. A process mean outside the declared deviation
band produces a diagnostic.

The report combines independent group sigmas by root-sum-square, with signed
mean contributions and an editable 0–10 sigma multiplier. This follows linear
uncertainty propagation when covariance between groups is zero; [NIST explains
why shared measurements require covariance](https://www.itl.nist.gov/div898/handbook/mpc/section5/mpc55.htm).
Shared groups here represent one common variation, including exact coefficient
cancellation. General covariance matrices and partial correlation are not
supported.

The reported statistical band is a stated k-sigma spread. It is not a worst-case
guarantee, capability index, Monte Carlo result or inferred process yield. Mixed
uniform/normal sums are not asserted to be normal. The [NIST normal-distribution
discussion](https://www.itl.nist.gov/div898/handbook/pmc/section5/pmc51.htm) describes
the additional distribution assumption needed to interpret normal sigma bands
as probabilities.

See the [editor workflow](tolerance-stack-editor.md) for field and source-repair details.

## Editing, reports and source

Fields expose semantic source identities, units, choices, errors and commands
independently of layout and styling. Invalid typed values remain visible for
correction. **Advanced Source** edits the same versioned definition. **Apply**
requires valid current source and a current available stack report, together
with ordinary current model validation. **Cancel** restores the shared starting
checkpoint; one Apply is one undoable operation.

A persistent background thread coalesces edits into one pending calculation and
one current reply. Reports are matched to the exact document fingerprint;
obsolete results are discarded. Navigation stays available. Reports contain f64
millimeter values and stable source references. Export the current JSON report
from the panel or use `tcad-cli tolerance-stacks PROJECT --output REPORT.json`.
CLI reporting reads source and configurations without rebuilding geometry.

The application-owned contract is `tcad.tolerance-stacks` version 1. A document
supports at most 128 stacks and 128 terms per stack, with bounded source text and
finite coefficients. The implementation handles linear authored dimensions;
automatic assembly-loop closure, measured 3D gaps, nonlinear propagation and
statistical sampling remain gaps. Implementation verification is compilation,
formatting and strict linting. Interactive acceptance remains with the user;
no code tests or UI smoke runs were added or performed.
