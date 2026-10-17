# Git, forks, component updates, and model previews

TCAD is a visual Git client for its native models. Use a local repository or a
normal Git server; a TCAD server and a forge account are not required. HTTPS,
SSH, and local Git repository addresses are supported. GitHub, GitLab, Forgejo,
Bitbucket, and other compatible Git servers use the same URL workflow. Hosted
repository search and creation currently have a GitHub.com adapter only.

## Open or create a repository

Open **Workspace → Git Repositories** or **File → Open Git Repository**.

1. Enter a Git URL or a local Git repository path. Choose existing Git credentials
   or a saved connection. Leave Branch blank to use the remote default.
2. Choose a new local folder and select **Clone repository**.
3. Choose a TCAD model from the downloaded working copy. TCAD finds named and
   nested project manifests. Existing folders and unsaved work are not replaced.

**Open a local folder** finds models in an existing checkout without cloning.
**Create a local repository** starts an empty repository with a chosen first
branch. It needs no account or remote. To copy the current saved model into a new
local repository, use **Save or publish current model → New local repository**.
Use **Versions** for later commits, branches, fetch, merges, and push.

Opening rechecks definitions and resource hashes. Scans skip hidden and build
folders, symlinks, and invalid model files; notices explain rejected projects
and scan limits. Git submodules are not downloaded.

## Connections and credentials

Open **Settings → Git connections**, **Workspace → Git Connections**, or
**Manage connections** in the repository panel. Existing Git credentials are the
default. A saved connection can instead use an HTTPS credential or a specific
SSH key, with a server scope. Cloned and published working copies remember the
chosen connection for later Versions operations.

For an existing checkout, expand **Current repository**. Read its settings,
choose a connection, and select **Use connection for this repository**. Choose
existing Git credentials to remove a saved account choice. You can also set the
commit author name and email for this repository without changing global Git
settings. This name identifies commits; it is separate from authentication.

- **HTTPS:** enter the server address, username, and password or access token.
  **Store credential securely** uses macOS Keychain. Save the connection, then
  select it when cloning or publishing. Other platforms can use their installed
  Git credential manager through the existing-credentials option.
- **SSH:** choose an existing private key or create an encrypted key. Read and
  copy its public key to register it with the Git host. Unlock the private key
  for the SSH agent when needed. Private keys stay on this computer.
- For a new SSH server, **Review server fingerprints**, check them with the
  server owner, then trust the reviewed keys. Changed keys block the connection.

Connection preferences contain names, scopes, and key-file paths. Passwords,
tokens, and passphrases are not saved in models or preference files and are not
exposed in semantic UI data. Connection and credential changes take effect
immediately; Settings Apply or Cancel does not undo them. Removing a connection
retains its key files and stored credential; removal of a credential is separate.

## Optional GitHub connection

Expand **Forge connections · GitHub** in Git Repositories. **Connect GitHub with
browser** shows a device code and opens GitHub. Complete sign-in in the browser.
You can browse and search accessible repositories, choose an owner, look up an
exact `owner/name`, switch saved accounts, or disconnect an account.

Selecting a repository puts its URL and GitHub connection into Open models.
**New private GitHub repository** opens a creation form. Each new form defaults
to private. You can create an empty repository without a saved model or create
one while publishing. Public creation needs explicit acknowledgment. Organization
policies can still deny creation.

The Mac bundle includes GitHub CLI **2.83.2** as `tcad-gh`, pinned by archive
checksum and signed with the app. No separate GitHub CLI install is needed. Git
and SSH must be available on the computer. Development runs can use `TCAD_GH`, an
adjacent `tcad-gh`, or an installed `gh` for this optional adapter.

GitHub CLI owns its account store. Account switching and sign-out also affect
GitHub CLI on this computer. It tries the system credential store and can fall
back to local configuration; TCAD reports that fallback. Environment tokens take
precedence and must be removed before changing sign-in. See the official
[GitHub CLI sign-in documentation](https://cli.github.com/manual/gh_auth_login).

## Publish a model

Save and finish the current model first. In **Save or publish current model**:

1. Choose a Git URL, a new local repository, or a GitHub destination. Generic
   URLs need no GitHub account. GitHub destinations show their checked visibility;
   Git itself does not report who can access a generic URL.
2. Choose a new local working-copy folder, branch, and model folder. A repository
   with existing files needs an existing branch and a new model subfolder.
3. Enter a version message and commit author. TCAD reads the current Git author
   settings; you can edit the name and email without changing global settings.
4. Review the saved source, destination, access, connection, branch, exact remote
   version, local folder, and author. For an unknown-access destination, explicitly
   confirm that you checked its access before publishing. Public GitHub publishing
   requires explicit public disclosure acknowledgment.
5. Publish, or create the local model repository. **Open saved working copy**
   continues editing there. Use **Versions → Save and commit**, then
   **Push current branch** for later remote updates.

Only the chosen model's manifest, definitions, referenced resources, shared
presentation, and current generated README preview are copied. The source model
stays unchanged. Its enclosing repository and unrelated files are not uploaded.
Existing remote files and history are retained; this flow does not replace an
existing model folder. Pushes are never forced. If the reviewed remote branch
changes, publication stops for a fresh review. Explicit GitHub destinations also
recheck account, repository identity, and visibility. An empty-branch review is
an empty-branch requirement, not permission to replace later changes.

Network work runs in the background with progress and cancellation. Cancellation
cannot undo a completed remote action. Created repositories and retained local
working-copy paths remain visible after partial failure. Check the destination
before retrying an interrupted upload. Late results cannot replace a model that
changed meanwhile.

Transfer limits are 100 MiB per file and 1 GiB for a model plus its shared
presentation. Git LFS and automatic submodule retrieval are not provided. Create
other branches in Versions after opening a working copy. Generic-host publishing
uses a thumbnail and downloadable STL preview; GitHub-only rendering markup is
not assumed for other servers.

The versioned forge, Git-connection, and model-transfer contracts are in
`cad-core`. The desktop supplies background jobs and semantic fields, actions,
review, and progress. Layouts and themes can arrange and style these controls.
Secrets are handled separately from the semantic control values. No manual UI,
live authentication, repository creation, clone, or publish acceptance checks
were run for this update. Interactive acceptance remains with the user.

## Versions

Open **Workspace → Versions**.

- **Save and commit** saves the applied model and records its project files.
  Unrelated staged files are not included in a normal project commit.
- **Create branch** starts another line of work with the same model identities.
  **Switch branch** requires a saved model and a clean checkout.
- **Fetch remote** downloads history. It does not change the open model.
- **Compare revision** shows source changes and an overlay of the other model.
- **Preview branch merge** uses the common ancestor and both branches. Apply
  requires a current, valid model evaluation. The next commit records both parents.
- **Push current branch** sends commits to the selected remote. TCAD does not
  force-push or change repository visibility.

The merge preview includes other changed repository files. Conflicts outside
the model stop the merge. Resolve these with Git, or transfer selected features
instead. Source conflicts are reported; TCAD does not choose a winner silently.
Switching, committing, fetching, and reading remote sources run outside the UI
thread. Stale results cannot replace a newer document.

A full merge records its branch and pending checkout changes before advancing
Git history. If checkout stops partway through, **Versions → Recover interrupted
merge** completes only those recorded files. Recovery stops if the branch or
affected local files changed. Other staged work is retained. Finish recovery
before saving to that project, switching branches, committing, or pushing;
Save As to a new project remains available.
Submodule changes and directory/file transitions require a Git client; the
recovery operation stops before these cases instead of guessing.

The project commit includes the native manifest, referenced definition and
resource files, saved workspace presentation, and generated README preview files.
Other source or documentation files can still be committed with a Git client.
Local recovery and cache directories are excluded from Git.

## Fork a part or assembly

Select the component, then use **Design → Fork Part or Assembly**. The existing
**Make unique** action uses the same operation.

A fork creates new identities and saves the relationship to the source. It does
not create a Git branch or a forge repository. Use **Save As** to put the result
in a separate project, then connect that project to its own Git repository if needed.

For an assembly, choose one of these options:

- **Reuse children** copies the assembly and keeps its current child definitions.
- **Fork selected children** also copies the chosen child definitions and their
  required enclosing assemblies.
- **Fork all children** creates independent copies of the full child hierarchy.

Select a specific occurrence to replace only that occurrence. When necessary,
TCAD copies its enclosing assemblies so another occurrence keeps its old design.
The review lists copied and reused definitions. Apply accepts one validated,
undoable operation. Cancel restores the starting model and editing context.

Linked component forks use the verified original source snapshot for ancestry.
The saved record contains a source definition, identity mapping, fingerprint,
and exact Git commit when known. It does not contain generated geometry or a
growing event log. Aggregate fork bases are limited to 32 MiB.

Pattern-generated occurrences must be detached before an occurrence fork.
Some occurrence-bound external modeling references and part-pattern directions
need explicit repair or detachment first. TCAD reports these cases instead of
silently changing their meaning.

## Reuse and update components

Open **Design → Component libraries**. Link a local project, or enter a Git
repository, project folder, and initial branch, tag, or commit. Inspect the source,
then choose its part or assembly and configuration. Preview the link and Apply.

A link stores an exact source commit or local source fingerprint. The project
contains the source snapshot and verified resources, so the pinned model can
rebuild offline. The imported definitions include the selected component's
dependency closure. Linked definitions stay read-only; fork them to make a
local variant.

**Check updates from** selects the Git branch or tag to watch when you press
**Check**. Checking does not change the pin. **Preview update** reports source
changes and affected component occurrences, mates, drawings, and BOM rows in
the open project. Inspect the result, then Apply or Cancel. A failed rebuild or
missing reference blocks Apply.

An accepted update changes all uses of that link in the open project. Other
projects keep their pins until their owners accept an update. There is no
background update across every assembly on the computer. If a local source
moves, use **Locate source** and review it before replacing the pin.

## Transfer selected features

Open the target project and choose **Design → Transfer Selected Features**.
Load a source project at a Git revision, leave the revision blank to use its saved
files, or use another definition in the current project. Select the source and
target parts, then select the required features.

The review shows each feature, sketch, parameter, and resource dependency.
Existing corresponding inputs can be reused. New required inputs are included.
You can bind an input to a local feature or sketch instead of importing its
upstream version. External parameter bindings check units. Explicit reference
repair uses picked target geometry.

When both sides changed an item, choose **Keep target** or **Use source** for
that item. The planner uses branch ancestry, fork correspondence, and saved
transfer bases. An unresolved conflict blocks preview. Existing target bodies
with later edits require an explicit current-body binding or an explicit choice
to keep separate results; a transferred cut does not silently create an overlapping
parallel body.

Preview evaluates the proposed model. Apply accepts one undoable change. Cancel
restores the original definitions. Only needed, hash-verified resources are copied;
unused staged resources do not become model definitions. Repeating a transfer
reuses the saved identity mapping and reviewed base.

The same workflow works in either direction: take a feature from upstream, or
open upstream and take a feature from a fork. It does not mark the entire source
branch as merged. Retained correspondence can be followed through multiple fork
generations. Missing ancestry requires explicit input bindings; conflicting
ancestry blocks the transfer. Feature deletion, feature-order reconciliation, and assembly
mate/instance transfers are not part of selective part-feature transfer. Use a
reviewed full branch merge for wider document changes.

## Interactive README preview

Save the applied model, then choose **Workspace → Update README Preview**.
Versions also has **Update preview before each commit**. This is a saved personal
preference. Generation runs in a persistent geometry worker and requires the
same source fingerprint as the saved model.

TCAD writes:

- `previews/model.stl`: the complete evaluated model, including assembly placement
  and assembly welds, in millimeters.
- `previews/model.png`: a compact thumbnail.
- `previews/model-preview.json`: source, backend, accuracy, units, and hash metadata.
- One marked section of `README.md`. Other README text is retained.

Small models use an ASCII `stl` code block that GitHub renders as an interactive
model in the README. Larger models use the thumbnail as a link to GitHub's
interactive STL file viewer. GitHub documents both [README STL blocks](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/creating-diagrams#creating-stl-3d-models)
and its [3D file viewer](https://docs.github.com/en/repositories/working-with-files/using-files/working-with-non-code-files).
Other forges can show the image and offer the STL download; their interactive
display depends on their own support.

The preview contains display geometry, not editable CAD source. STL does not
retain finishes, textures, feature history, or constraints. Inline output is
limited to 180 KB; binary output is limited to 180,000 triangles to stay below
GitHub's file-viewer limit. TCAD does not discard random triangles. If the model
exceeds the limit, increase display chord tolerance and generate it again.
Released source folders require Save As before changes.

## Headless use and contracts

The CLI provides `fork`, `transfer-features`, `link-git`, and `readme-preview`.
The first three accept JSON requests and review without writing the model by
default. `--apply` evaluates the candidate before saving. Source changes during
evaluation prevent the save. `transfer-features --revision` resolves the source
to one immutable commit; `--base` supplies an explicit branch merge base.

The normative request types are `component_fork::Intent`,
`feature_transfer::Request`, and `library::GitLinkRequest` in `cad-core`.
Fork lineage, transfer receipts, and README metadata use contract version 1.
The native schema remains 1 with additive default-empty fields. Worker protocol
3 adds README preview requests; desktop and worker must use matching versions.
No kernel or renderer objects are saved in these contracts. The UI exposes
semantic controls through `component-fork`, `feature-transfer`, `git-ui`, and
`readme-preview` adapters.

## Validation

On 2026-10-17, the forge-neutral repository and connection update passed workspace
format checking, strict Clippy for all libraries and executables, whitespace
checking, and package-script syntax checking. This includes the new SSH
passphrase helper. No tests, live credential operations, repository transfers,
app launches, or manual acceptance checks were run. The implementation retains
the existing dependency future-compatibility warning noted below.

On 2026-10-16, implementation checkpoint `14a0dcc` passed:

- `cargo fmt --all --check`
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`
- `git diff --check`
- `scripts/package-macos.sh`, including locked workspace compilation, bundle
  property-list linting, and strict signatures for the desktop, worker, CLI,
  and completed `dist/TCAD.app`.

Cargo reported an existing future-compatibility warning for dependency
`proc-macro-error2 2.0.1`. No code tests, probe scripts, app launches, or manual
smoke checks were run. These checks establish source integration and packaging;
interactive acceptance remains with the user.
