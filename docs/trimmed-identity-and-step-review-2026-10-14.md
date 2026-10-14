# Trim identity retention and STEP source review

Implementation and static review on 2026-10-14. No tests, assertion probes, or manual UI checks were performed.

## Native trim metadata

The Monstertruck trimmed path now retains the metadata already supported by its ordinary topology path:

- `CompressedTrimmedShell` carries optional aligned vertex, edge, and face stable-ID arrays with serde defaults for older serialized data.
- `CompressedTrimmedSolid` carries optional stable-ID allocator state and sparse element attributes.
- Native and compressed trim attachment, compression, trim erasure, and plain/trimmed conversion preserve these values.
- `TrimmedSolid::try_mapped` maps one-to-one shared topology and each trim, rejects failed mappings or invalid resulting boundaries, and retains allocator/attributes.
- `TrimmedShell::try_mapped` restores source edge/face stable IDs after ordinary shell mapping, which otherwise creates new unassigned edge/face IDs. Mapped vertices already retain their stable IDs.
- Trimmed extraction rejects wrong ID-array lengths, repeated assigned IDs within a collection, aliasing of different topology elements to one assigned identity, invalid indices, and exhausted allocator values. It advances the allocator above retained topology IDs when needed. Missing optional metadata remains compatible.

Face inversion leaves per-use trims in the absolute carrier frame. Compression and extraction preserve the absolute wire-use order, edge-use orientation, and separate face orientation, so inversion does not require rewriting the saved UV curves.

The generic unchecked constructors remain unchecked. These metadata changes do not certify UV loop validity, global solid enclosure, or carrier intersection correctness. Existing healing passes can change topology without maintaining aligned stable-ID arrays; malformed retained metadata then fails trimmed extraction instead of being silently truncated. A general healing provenance/attribute migration remains separate work.

## STEP orientation and parameter frames

The modeling-surface STEP orientation adapter now distinguishes emitted carrier forms. A recognized analytic cylinder combines native profile axial direction, placement determinant sign, and processor UV transposition to choose its STEP `same_sense`. Rational fallback retains `same_sense=true` because the emitted net already carries its transform and orientation. Generic unreifiable revolution fallback accounts for its parameter-direction exchange and reflected placement.

Static source review confirms the sphere pcurve import maps STEP longitude/latitude to native colatitude/longitude for both processor orientations. Cylinder/cone loading already transposes the native revolution frame, so their preserved imported UV curves keep that established parameterization. The saved association gate omits pcurves when a carrier serializer changes parameterization without providing a coordinate map.

The raw formatters now reject implicit intersection search guides as 3D boundary authority and refuse direct parameter curves when a carrier formatter changes their UV frame. TCAD materializes supported boundaries through its bounded exchange path before formatting; arbitrary implicit branches and nonrepresentable parameter curves remain explicit refusals. Assembly STEP uncertainty now emits the required typed LENGTH_MEASURE value, matching the standalone exporter.

Healing's sampled `same_surface`/trim-association helpers remain insufficient as exact carrier or branch proof. They should not become the admission authority for the application-owned trim contract.

## Evidence

Local-atlas and loft changes passed strict kernel lint against the integrated dependency checkout before subsequent concurrent work. The trimmed topology library passed isolated strict lint with default features disabled. Two preexisting mutable-key lint sites in shell adjacency have narrow documented allowances: `Face::Hash/Eq` use only the shared allocation address and orientation, which mutable geometry cannot change while a key is immutably borrowed. Default-feature topology, IO and healing strict lint passed under Rust 1.99.0. The fork's floating nightly encounters an existing `branches` intrinsic compatibility issue; explicit Rust 1.99.0 avoids that separate toolchain problem. Final integrated build/format/lint evidence belongs to the combined checkpoint.
