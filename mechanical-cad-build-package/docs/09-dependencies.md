# Starting dependency candidates

Research checked for this handoff on 2026-10-10. These are starting candidates, not a compiled compatibility set.

All candidates are initial replaceable implementations. Resolve vendor dependencies inside adapters and select them through composition code. Document contract, capability, lifecycle, and migration effects when replacing any candidate. The UI layout/style system is separate from the chosen widget/rendering toolkit.

| Component | Candidate | Use |
|---|---|---|
| Rust | 1.99.0, edition 2024 | Core and native desktop implementation |
| Bevy | 0.20.0 | Shell, scene, viewport, input, UI rendering |
| Monstertruck | 0.4.1 | Modeling, solid operations, tessellation, STEP |
| UI behavior | Pinned Bevy headless widgets; semantic bindings and resource-defined visuals | First experiment, subject to manual field/focus demonstrations |
| Constraint solvers | Open | Sketches and assembly placements |
| Source serialization/IPC | Open | Small explicit versioned contracts |

Rust 1.99.0 is documented in the [official release announcement](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/). Pin the toolchain proven by M0 rather than tracking an unversioned stable alias.

Bevy 0.20.0 was announced October 8. It includes headless tabs, expanded editable/scrubbable fields, and a pan-orbit camera. These are useful probe targets; they do not replace application-owned trackpad routing or the user-defined layout/style contract. See the [official release](https://bevy.org/news/bevy-0-20/) and [UI widget documentation](https://docs.rs/bevy_ui_widgets/latest/bevy_ui_widgets/). Pin version-specific API references once resolved.

Monstertruck 0.4.1's facade has modeling, meshing, solid, and STEP features. Use the geometry features without its renderer because Bevy owns the viewport. The solid feature includes fillet/healing integrations. See [published features](https://docs.rs/crate/monstertruck/0.4.1/features) and the [upstream repository](https://github.com/virtualritz/monstertruck). STEP has moved to monstertruck-io; prefer the current facade instead of building on the deprecated monstertruck-step package. [Published deprecation note](https://docs.rs/crate/monstertruck-step/0.4.1/source/Cargo.toml).

## Candidate manifest input

[dependency-candidates.toml](../implementation/dependency-candidates.toml) records the versions and proposed features for the implementation agent. It is planning input, not an application Cargo manifest or a generated lockfile. M0 must confirm feature names, minimum compiler requirements, and actual resolution.

Prefer a single locked set of Monstertruck sub-crates. If depending on individual crates, keep their versions aligned. Avoid adding a second GPU renderer, a Python runtime, or a web UI stack for this first build.

## UI strategy

Probe native Bevy UI with headless behavior and semantic bindings first. A separate user-editable layout/style layer produces the default flat rail visuals or other compositions. Keep widget/rendering APIs inside replaceable view adapters. A generic node editor must not dictate the available semantic data or the user's visual composition.

Keep the widget decision open until exact numeric entry, expressions, focus, text selection, clipboard, IME where required, scaling, accessibility, and scrubbing work well. If a different field toolkit is needed, record the choice and keep domain ownership unchanged.

## Licensing and assets

Original code is intended for WTFPL; install its official license text with the project's actual copyright ownership when source is created. Preserve dependency licenses and notices. Monstertruck retains Apache-2.0. Audit the resolved dependency tree and production asset licenses during M0.

Use original or appropriately licensed fonts, icons, textures, and environment maps, with recorded provenance. The concept renders are references rather than a source of production material maps.

## Verification status

The handoff preparation verified primary-source release information and package structure. It did not compile a Bevy/Monstertruck application, resolve a lockfile, evaluate fixture geometry, or perform hardware input sessions. Those tasks have explicit M0 acceptance criteria. Do not write code tests; manually invoked capability demonstrations and runtime validation remain required.
