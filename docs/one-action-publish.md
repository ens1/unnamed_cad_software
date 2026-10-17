# One-action Publish

Publish creates a manufacturing package from the saved project, commits the
model and generated files together, and pushes the commit to the chosen Git
repository and branch. Design review and merging into `main` stay in the user's
forge. TCAD does not create a separate release system or automatically merge a
feature branch into `main`.

## Project choices

The first click on Publish opens its options. Choose the destination, output
formats, connection, and Git author, then click Publish in the panel. TCAD asks
where to save a new project if needed. Later, the main Publish button uses the
saved choices in one action. File → Publish Settings opens the options again.
While a publication runs, the Publishing button reopens its progress and Cancel
controls.

The Publish panel remembers the repository URL, branch, folder inside the
repository, and selected output formats in `tcad-publish.json` beside the native
project manifest. This is a versioned, human-readable project file. It is
included in the publication so the choices travel with the model. Credentials,
connection secrets, and local checkout locations are not stored in it.

The selected connection and Git author are remembered per project on this
computer. They are separate from the shared project file and do not change the
source repository's connection or Git author configuration. New projects start
with the available Git author details; authentication remains with the existing
Git Connections and operating-system credential tools.

The default outputs are:

- Drawing PDF, containing all saved sheets.
- Drawing DXF, containing the drawing geometry.
- BOM CSV, using the project's production settings.
- STEP, containing the evaluated model.

STL, sketch DXF, sheet-metal flat-pattern DXF, and structural cut-list CSV can
also be selected. Output filenames are stable and live in `outputs/` inside the
published project folder. Flat patterns and cut lists require applicable model
definitions; a selected output that cannot be generated stops publication with
an explanation instead of silently omitting the file.

If the model has no saved drawing sheets, the existing drawing exporter creates
an A4 sheet with front, top, right, and isometric views of the root model. This is
an automatic projection, without invented dimensions or manufacturing
tolerances. The Publish panel and `publication.json` identify this fallback.
Create drawing sheets in TCAD when dimensioned production drawings are needed.

If the project already has a TCAD README model preview, Publish regenerates that
preview from the same evaluated model. It replaces only TCAD's marked preview
section in an existing repository README and preserves the surrounding text.
Projects without a saved preview do not receive one automatically.

## Source and generated files

Generation runs in a dedicated persistent geometry worker. TCAD freezes the
committed model, exact pinned resource bytes, shared presentation resources, and
Publish choices into a new local staging folder. It checks this frozen source
and evaluates it before writing any publication package.

`publication.json` records the document identity, source fingerprint, revision,
application and geometry adapter versions, modeling and export accuracy,
selected outputs, drawing source, and SHA-256 hashes of generated files. Export
quality uses the project's display/export chord tolerance. Native modeling
accuracy remains separate.

Every selected file must succeed before the completed package is handed to the
Git adapter. A model error or export error does not create a partial remote
publication. The source model and generated outputs are committed together;
they cannot land in separate commits through this action.

## Git and recovery

The Git adapter uses an isolated local checkout for publication. It does not
switch the working model's branch or stage unrelated files from the user's
working checkout. It stages an explicit inventory of source and generated
files. An ownership record in the published project folder identifies
the files managed by this action so later publications can update or remove
their own old outputs without deleting unrelated repository files.

Publication preserves the source repository's committed history when it belongs
to the chosen repository. Compatible source and target histories can be joined
inside the isolated checkout when a previous successful publication to that
same destination supplies a baseline and the remote managed project files have
not changed since it. The exact validated CAD snapshot supplies the managed
model files. Remote changes to those files and conflicts in unrelated files
require an explicit merge before publishing. This process does not run merge
drivers or hooks, and it does not change the working model's checkout or index.

Push uses the chosen branch without force. A changed remote branch, conflicting
project, modified managed output, or failed connection is reported. A completed
local publication commit is retained when pushing fails. **Retry saved upload**
sends that same commit without rebuilding the geometry or creating another
commit. Publishing the same unchanged model with the same choices also reuses
that waiting commit.

Local source snapshots, generated packages, and isolated publication repositories
live under `~/.tcad/publishing/jobs/`. Per-project receipts and machine-local
Publish choices live under `~/.tcad/publishing/`. These files preserve the exact
commit and output package needed for recovery; they are not written into the
working model's source folder. Show published files opens the retained checkout.

Generation and network work keep viewport navigation responsive. Modeling edits
are held during publication so the exact source being published stays clear.
Generation has a ten-minute worker deadline; ordinary geometry requests retain
their existing two-minute deadline. Cancelling before push leaves the remote
unchanged. Cancellation or a connection loss during push can have an uncertain
remote outcome. Retry saved upload checks the remote branch first: if that exact
commit is already there, it records success. Otherwise it pushes only when the
saved remote baseline still matches. It does not overwrite a newer remote
branch to resolve uncertainty.

## Validation policy

Implementation checks are compilation, formatting, and linting. No automated
code tests, assertion probes, or manual smoke tests are introduced or run.
Interactive acceptance remains with the user.

Implementation validation on 2026-10-17 passed workspace compilation,
`cargo fmt --all --check`, strict workspace library/binary Clippy with
`-D warnings`, and `git diff --check`. `scripts/package-macos.sh` rebuilt the
workspace and packaged `dist/TCAD.app`; the bundle and nested executable
signatures passed verification. The existing `proc-macro-error2` future Rust
compatibility notice remains. No publication, model operation, or app session
was run for acceptance.
