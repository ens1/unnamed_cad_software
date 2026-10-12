# Personal sketch-block library

The Tools menu and command palette open a searchable personal library of native
`.tcadblock` copies. Add block files, filter by category or tags, mark favorites,
and save a block from the current sketch. Selecting a browser entry changes only
the browser. Its source document, imported file and active sketch are unchanged.
No sample block is inserted automatically.

An active sketch is required for **Insert block copy**. The browser verifies the
chosen entry and its pinned bytes, then opens the existing native block insertion
operation in that same sketch and viewport. Translation, rotation, scale and
accept/cancel continue through the shared sketch editor and its current preview.
Insertion copies and namespaces the native definition and provenance through the
existing import adapter; it does not create a live external file dependency.

**Save current block copy** snapshots the selected local block and its actual
configured sketch source into a native source package. The local block picker
makes the choice explicit when the sketch contains several blocks. Unfinished
viewport gestures, invalid sketch edits and malformed or unapplied Advanced
Sketch Source disable this action and insertion. The raw source buffer tracks its
last accepted baseline: an untouched buffer follows native drawing and solver
changes, while actual raw edits are preserved until explicit Load or Refresh.
Legacy buffers with unknown baselines remain conservative. Browsing and closing
restore the previous editor context without loading or discarding pending source.

## Persistence and admission

The personal library lives in `sketch-blocks` beside the desktop preference file.
Its version 1 `index.json` records independent entry IDs, searchable labels,
categories, tags, favorite/archive flags, exact encoded-file SHA-256, native
canonical package SHA-256, source document digest and local identities, and
member/point/constraint counts. Source copies live at digest-named
`blocks/<sha256>.tcadblock` paths. Metadata renaming does not alter these copies.
The library is local to the user; inserting a copy pins its native source into the
project through the established sketch-block resource contract.

Imports validate the native package before writing and admit at most 512 indexed
entries and 128 MiB of indexed pinned source bytes, including removed entries.
Each package is bounded to the existing 16 MiB native resource limit. Existing
native 512-point, 128-relation and 16,384 solve-work admission limits still apply;
a browser entry does not relax solver limits. The JSON index has a 4 MiB read
limit. Reads check size before allocation, and pinned source reads verify exact
bytes, canonical identity, provenance and counts. A changed or missing file,
invalid index, archived insertion target or stale entry/source/metadata revision
produces a diagnostic; the application never guesses an original source path.

A process-safe file lock serializes mutations. Atomic recoverable writes save
source copies before the index, and metadata updates compare the current revision
before writing. Re-adding identical pinned bytes selects the same entry and can
restore it. Remove archives an entry and retains its bytes; Show removed copies
provides Restore and Export. Archived entries still consume admission capacity.
Export writes the exact verified native source bytes atomically. Crash orphan
blobs are not automatically collected and are outside the indexed byte count;
manual management of this local directory is separate from project resources.

## Presentation contract

`workspace.sketch-block-library` exposes the `sketch-block-library-editor`
renderer, versioned library contract, limits and location. The semantic collection
includes searchable metadata, source provenance, digests, counts, archive state,
favorites and explicit insertion availability. Commands are
`sketch-block-library` and `sketch-library.add`, `.refresh`, `.save-local`,
`.select`, `.insert`, `.favorite`, `.remove`, `.restore`, `.export` and `.close`.
Fields use the `sketch-block-library` setter, stable document/entry/source editor
identities and revision-checked metadata mutations. Rejected label/category/tag
text remains available for correction, including when another valid field changes.
No library metadata change participates in modeling Apply or changes geometry.

All browser fields, actions, rows, diagnostics and native source facts are
independent of default layout and exposed to user styling. The default is a
compact scrollable browser with a configurable CSS-like `max-height`. This work
adds an index and browser to the existing portable native import/export workflow;
shared network libraries, live external block updates and filesystem watching
remain separate future capabilities. Compilation, formatting and strict linting
validate the implementation. No tests, probes or interactive smoke runs were
written or performed.
