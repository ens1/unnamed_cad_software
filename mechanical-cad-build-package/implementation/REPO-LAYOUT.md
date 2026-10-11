# Proposed repository layout

The package is the documentation/design seed. Create source crates during M0; add later areas as milestones require them. This is an initial organization, not a permanent implementation commitment. Every subsystem uses a replaceable application-owned contract, with implementation selection in composition code.

~~~text
Cargo.toml
Cargo.lock
rust-toolchain.toml
AGENTS.md
docs/
assets/
crates/
  cad-core/
    src/
      ids.rs
      units.rs
      document/
      commands/
      evaluation/
      references/
      persistence/
      contracts/
      presentation/    # theme-agnostic projections and command descriptors
  cad-kernel/
    src/
      adapter.rs
      monstertruck/
      provenance.rs
      tessellation.rs
      validation.rs
      exchange.rs
  cad-worker/
    src/
      main.rs
      protocol.rs
      server.rs
  cad-desktop/
    src/
      main.rs
      input/
      viewport/
      rail/
      layout/
      styling/
      view_adapters/
      composition.rs
      worker_client.rs
  cad-cli/             # introduce when headless workflows need a public CLI
fixtures/
probes/
~~~

## Dependency direction

- cad-core depends on small domain/serialization/error libraries, not Bevy or Monstertruck.
- cad-kernel depends on cad-core and the selected Monstertruck geometry crates.
- cad-worker depends on the core and kernel adapter.
- cad-desktop depends on cad-core and Bevy; it communicates with the worker.
- Semantic UI projections contain no Bevy types, widget handles, or visual styling. The default rail and user layouts bind to these same projections and commands through view adapters.
- Layout and styling resources have independent schemas. Composition, widget, renderer, kernel, transport, and persistence implementations can be replaced without consumer access to vendor internals.
- cad-cli uses the same domain/evaluation path and worker or adapter boundary. It must not implement a separate modeling engine.

A shared IPC envelope can live in a small core protocol module initially; extract it into a separate crate only when that boundary benefits from independent versioning.

Input normalization can start as pure modules in cad-desktop. Isolate platform adapters and keep semantic gesture types independent of Bevy event types and fixed layout coordinates.

## Bootstrap

Begin with the minimal kernel probe and Bevy viewport. Add document and transport code as the probes establish the correct types and supported operations. Do not create dozens of empty crates to resemble the final architecture.

Store manual demonstration outputs in disposable directories. Commit source examples and selected observed artifacts, not full build targets or geometry caches. Do not write tests for any code or substitute automated assertion suites.

After the workspace exists, record the real commands in the README and use locked builds. Pin the tested compiler and dependency set. The candidate planning file in this package is not a substitute for Cargo.lock.
