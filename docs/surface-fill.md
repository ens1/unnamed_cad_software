# Exact four-sided surface filling

Filled Surface accepts four ordered, end-connected open sketch curves. Each
source sketch supplies one non-construction native member. Lines, polynomial
B-splines, circular/elliptical conic arcs and positive-weight rational B-splines
can participate in the same fill. Boundaries can lie on different sketch
planes. The first boundary retains its traversal direction; subsequent curves
reverse when needed to join the preceding endpoint. Reverse face orientation is
an explicit saved option. Endpoints must meet within the native 0.0000001 mm
endpoint tolerance; gaps are diagnosed rather than bridged with new curves.

The native patch uses the Euclidean Coons sum

```
S(u,v) = (1-v) C0(u) + v C1(u)
       + (1-u) D0(v) + u D1(v) - B(u,v)
```

where `B` is the bilinear corner patch and opposing boundary curves are oriented
in the same parameter direction. Source knots are clamped and normalized to
the complete directed boundary interval. Opposing boundaries need neither the
same degree nor matching interior knot partitions. Native knot insertion,
degree elevation and Bernstein products preserve their curves rather than
fitting to display samples.

Variable rational weights no longer cause a polynomial-only rejection.
Opposing homogeneous boundaries are multiplied by each other's positive
denominator, giving each opposing pair the same weight function. The Coons
numerator is then formed algebraically over the product of the horizontal and
vertical denominators. Linear factors and the bilinear correction are
multiplied in the actual Bernstein basis. Rational weights therefore do not
distort the Euclidean blending of a line with an arc or a polynomial spline with
a rational spline. Genuine constant-weight inputs retain the smaller
polynomial construction; nearly equal weights are not silently discarded.

The result is one native rational face with four natural boundary curves.
Internal native span rows share controls, and all four corners share native
vertices. Its face and edges have construction identities derived from the
authored sketch/member sources, member pieces, boundary order and traversal
direction. Boundary representations can gain degree and denominator factors;
their curve loci are retained to native floating-point/endpoint precision.
Source definitions remain authoritative. The result also has the natural
rectangular carrier expected by parameter-domain Trim and Untrim.

The former all-pairs derivative-control-hull normal check has been replaced by
the surface adapter's bounded fixed-projection certificate. Every accepted
complete face has a proven injective projected rational Jacobian. Differentiable
carriers use positive principal minors. Position-continuous carriers with
nonsmooth interior knot joins retain the stronger positive-definite symmetric
Jacobian certificate. Bernstein proof subdivision refines proof coefficients,
not replacement geometry; all native cells retain the same projection frame.
This admits legitimate shear and some nonconvex control nets that the previous
gate rejected, while uncertain, singular, folded or exhausted cases remain
explicitly unsupported. A failed certificate does not establish that the
source design is folded.

Bounds are applied before expensive compatibility/net construction:

- Each boundary has degree one through sixteen, at most 4096 controls and finite
  positive weights within the recorded `1e-12` conditioning ratio.
- With variable rational weights, each opposing source-degree sum plus one must
  be at most sixteen. This includes quadratic conics and cubic rational curves.
- The complete synchronized boundary partition has at most 256 native Bezier
  cells, and the resulting carrier has at most 32768 controls.
- The completed rational carrier must also retain the positive-weight
  conditioning ratio. Its global proof uses the shared limits of eight million
  coefficient operations per face and 32 million per operation; bounded scalar
  subdivision can reject an uncertain proof before those limits are reached.

General boundary networks, multi-member source wires, interior guide/point
constraints, tangent/curvature constraints, periodic seams, collapsed sides and
arbitrary trimmed model-face boundaries remain additional work. This is a
four-sided positional fill, not a claim of general Boundary Surface parity.
The existing docked editor, semantic operation fields, shared draft/undo,
worker evaluation, current-validation Apply gate and Cancel checkpoint serve
the expanded geometry without introducing a separate transaction path.

Kernel implementation replacement invalidates derived caches. Source schema
and saved Fill intent are unchanged, but downstream references created by the
older location-based boundary naming can need explicit replacement. Geometry
acceptance remains with the user; implementation verification uses compilation,
formatting and linting without tests, assertion probes or UI smoke checks.
