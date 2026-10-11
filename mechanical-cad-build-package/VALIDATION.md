# Package validation

Prepared on 2026-10-10.

## Current repository revision

The handoff now records SolidWorks mechanical CAD as the minimum feature baseline. Current [root instructions](../AGENTS.md), [product scope](docs/01-product-spec.md), and [coverage inventory](../docs/mechanical-cad-coverage.md) supersede the original milestone-first/probe workflow and rail-only default. The default is empty Part1 with familiar CAD commands, a feature tree/property manager, and one model viewport; Flow is optional.

This requirements update used source inspection and official documentation. No application features were added, and no code tests, probes, or manual smoke tests were run. Interactive acceptance remains with the user. Manifest hashes/sizes are regenerated for the current package contents; the original checks below are historical evidence, not new verification results. The package references the surrounding TCAD repository and is not a standalone parity deliverable.

## Original handoff checks

- 7 PNG references checked for valid headers and dimensions.
- 7 JSON files parsed successfully.
- 3 fixture definitions, 6 feature nodes, 5 parameters, and 3 instances checked.
- Definition/instance identities, node/output links, parameter references, promoted controls, graph acyclicity, and shared-instance draft targets checked.
- 48 local Markdown references checked before packaging.
- Portable relative paths used throughout bundled documentation.
- Final manifest records SHA-256 and byte size for each other file.
- ZIP CRC integrity and local links checked after extraction into a different temporary directory.

## Implementation verification still required

This is a specification/design/build handoff. It contains no application Rust source, compiled binaries, Cargo.lock, or evaluated CAD outputs. The dependency candidates have primary-source research but have not been compiled together. Example geometry and topology resolution policies are illustrative. No physical gesture or cross-platform usability sessions were performed. M0 explicitly assigns manual demonstrations before implementation claims are made. Do not write tests for any code; retain build/lint checks and runtime validation.

## Specification revision

Updated on 2026-10-10 for independently replaceable subsystems, theme-agnostic UI information/actions with separate user-defined layout/style resources, and the policy against writing code tests. The instrument rail is now a bundled default. Build instructions, milestones, backlog, dependency policy, and UI references follow these changes.

Added an architectural contract document and illustrative layout/theme resources. These fixtures describe the intended interface; no layout engine, style parser, or alternate backend has been implemented. Original ZIP and semantic fixture checks above describe the original handoff and are not new verification claims.

Revision integrity checks: 10 JSON documents parsed (including the manifest), dependency-candidate TOML parsed, and 61 local Markdown links inspected with no missing targets. The manifest was regenerated with current file sizes and SHA-256 hashes, including the new resources. No code tests were written or run.
