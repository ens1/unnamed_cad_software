# Certified native parameter-curve export

The public Monstertruck fork now exposes
`Curve::try_lift_parameter_curve_with_error() -> Option<(Curve, f64)>`.
The distance is a complete world-space upper bound against the original native
parameter curve. Export/admission callers compare that bound against the saved
modeling residual. Display tessellation or Boolean search resolution is not an
acceptable substitute for this residual.

The implementation supports clamped positive rational planar curves and
clamped polynomial/rational tensor-product cells. It uses original plane
defining points, original spline controls and knot values, outward knot
extraction, complete cell containment, Bernstein composition and a positive
denominator bound. Final 3D control rounding and shared-piece homogeneous
scaling/join error enter the returned distance. Degree-one cross-cell pieces
are split only when the original outward root identifies an exactly
representable native parameter. Other uncertain crossings remain unsupported.

The convenience lift refuses unless its complete bound fits native numerical
precision. Complete normalized-parameter distance bounds additionally include
lift and subdivision error. Their upper bounds establish acceptance only when
small enough; a larger bound does not prove distinct loci or invalidate
different traversal speeds.

The public implementation and documentation contain generic dependency
geometry work, with no private application definitions or project models.
The native STEP caller owns final endpoint checks, dual-carrier/trim agreement,
branch restrictions and exported carrier UV-frame compatibility. A useful lift
does not by itself certify those independent conditions.

Verification: `cargo clippy -p monstertruck-modeling --lib -- -D warnings` passed
through TCAD's dependency resolution. Targeted Rust formatting was performed.
No tests, assertion probes, runtime examples or UI/manual smoke checks were
performed. The fork also compiles and strictly lints with Rust 1.99.0. Its floating
nightly toolchain encounters an existing `branches` intrinsic compatibility
issue; the explicit 1.99.0 toolchain avoids that independent build path.
