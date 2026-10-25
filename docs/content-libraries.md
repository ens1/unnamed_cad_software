# Content Libraries

TCAD ships with Classic, one basic aluminum engineering material, and one plain
clear-anodized display finish. No profile catalog, hardware catalog, additional
material catalog, texture image, finish preset collection, or extra theme is
embedded. The renderer keeps its ordinary studio lighting fallback. Existing
model files retain their saved material and appearance definitions.

## Install and use

Open **Workspace → Content Libraries** or **Settings → Content Libraries**.
The panel can open during a modeling operation without replacing its draft.
Library actions save immediately as personal preferences. Settings Cancel does
not undo a clone, library registration, commit, or update.

1. Choose a download connection with access to the source repository.
2. Click **Download … pack** for the content you want. Missing-library screens
   offer the same action and keep the current modeling draft intact.
3. The pack is cloned, registered, and enabled. Its entries appear in the normal
   profile, appearance, theme, material, and standard-part controls.

The default library folder is **`~/TCAD/Libraries`**. Each starter pack has a stable
subfolder, such as `tcad-weldment-profiles` or `tcad-textures`. The folder is created
when needed. **Choose folder…** and **Save library folder** set a different location
for future downloads; existing libraries stay in place. **Show library folder…**
opens the saved location. Each pack shows its source and destination, with
**Open repository…** and **Copy Git URL** actions.

An existing matching checkout is reused, including a disabled installation or a
completed download that was not registered. Downloads do not overwrite an occupied
folder or silently replace a fork. New transfers use a staging folder under
`.downloads`; failed or cancelled transfers are retained there. Retry starts a
fresh transfer. Status and failures remain visible in the pack section.

For other sources, enter a Git URL/local path and a new working folder under
**Other repositories**, then choose **Clone library**. An empty branch uses the
source default. **Use an existing local library** registers a clone you already
have. Enable its registration to make the entries available.

The initial sources are private and require access:

| Content | Repository |
| --- | --- |
| Structural profiles | [tcad-weldment-profiles](https://github.com/ens1/tcad-weldment-profiles) |
| Grayscale maps and 80 finish presets | [tcad-textures](https://github.com/ens1/tcad-textures) |
| Nocturne, Fusion 360-inspired, and SolidWorks-inspired themes | [tcad-themes](https://github.com/ens1/tcad-themes) |
| Engineering materials | [tcad-materials](https://github.com/ens1/tcad-materials) |
| Standard hardware | [tcad-standard-parts](https://github.com/ens1/tcad-standard-parts) |

No repository is installed automatically. Generic Git hosts and local Git paths
work through the same connection contracts as model repositories. Each installed
library has a local checkout and a personal registration, outside CAD documents.
Removing registration retains that folder. Removing a source does not delete
resources already copied into a saved project.

## Working without a pack

Missing catalogs show a short explanation, the relevant download action, and a
link to manage libraries. Existing project definitions and custom authoring remain
available. Structural Member checks for an available profile before completing an
active sketch. An empty catalog does not become a modeling error.

Missing legacy grain displays the saved finish color and optical response without
grain. The Appearance panel explains how to download the map or choose another
image, and ordinary appearance edits remain available. The saved map reference is
retained. Missing or corrupt images that are already pinned into a project still
need repair; TCAD does not claim that an incomplete portable asset is valid.

If an active theme checkout disappears, TCAD uses its saved personal snapshot when
available, otherwise Classic. The theme panel explains the fallback and provides
download and library controls. No download or pack installation changes the active
theme or committed model automatically.

## Own and edit a library

**Create local library** initializes a Git repository and a versioned manifest.
Catalog kinds start with editable empty JSON catalogs; themes start with a package
that inherits Classic. Finish libraries start with an editable neutral finish;
image libraries start empty. The working folder
must be new. Enter a Git author name and email for saved versions.

**Fork selected library** copies committed history into a new folder. It retains
its source for update inspection but has no publishing destination. The fork
starts disabled because its content IDs may match the original. Disable the
original before enabling the copy, or give independently maintained catalogs new
IDs. Duplicate identities across enabled catalogs are reported rather than
silently selecting a source.

Use **Show library files** to edit files with your preferred editor. Copy or export
new files into that folder, then add their type, ID, display name, and relative
path using **Add or replace entry**. Removing an entry leaves its file on disk. Ignored, untracked files stay outside
library snapshots and commits; a manifest cannot reference an ignored file until
you make it part of the repository.
Refresh after external edits. Use **Inspect changes** and **Save reviewed version**
to commit. Independently staged Git work is preserved; resolve it in your Git tool
before asking TCAD to save a complete library version.

The library manager edits the manifest and manages Git; it does not provide a
spreadsheet editor for every catalog schema. Existing profile, appearance, and
presentation authoring/export tools remain available. Catalog format details
remain in the library READMEs and application-owned contracts.

## Updates and publishing

**Inspect source update** fetches a separate checkout. Review added, changed, and
removed paths plus the old/new commit IDs, then apply explicitly. TCAD checks that
the reviewed source and candidate have not changed. It retains the previous
checkout. Uncommitted or staged work prevents update application. An update must contain the
current local commit in its history; divergent fork changes require a Git merge
or a separate source registration. Updating a library changes
future choices; it does not silently replace features, materials, finishes, or the
currently active theme in a model. Profile replacement is an explicit normal
preview/Apply action. Select an updated theme explicitly in Settings.

Create an empty private repository on the Git host of your choice, then enter its
URL and branch under **Publish library**. **Review destination** inspects the local
version and remote branch. Confirm who can access that destination, then publish.
Git URLs do not report visibility. Publication uploads the committed files **and
Git history**. Pushes use normal fast-forward rules and reject changed review
inputs. They never force-push. Keep new hosted repositories private unless you
choose to share them.

## Data contract and boundaries

`tcad-library.json` uses version 1:

```json
{
  "version": 1,
  "id": "workshop-library",
  "name": "Workshop library",
  "description": "Original workshop content",
  "entries": [
    { "id": "profiles", "name": "Workshop profiles", "kind": "weldment_profiles", "path": "profiles.json" },
    { "id": "brushed", "name": "Brushed height map", "kind": "textures", "path": "maps/brushed.png" },
    { "id": "theme", "name": "Workshop theme", "kind": "themes", "path": "theme/presentation.json" }
  ]
}
```

Kinds are `weldment_profiles`, `textures`, `themes`, `materials`, `standard_parts`,
and `finishes`. Catalogs retain their own schema versions. A texture entry points
to a supported raster image; HDR/EXR entries appear in the lighting picker. A theme
entry points to a presentation-package JSON. A finish entry points to a portable
`.tcadfinishes` package with pinned images. Textures and finishes remain independent
of engineering material properties.

Manifests and images use bounded reads, relative paths, SHA-256 fingerprints, and
explicit supported data types. Symlinks, escaping imports, executable payloads,
submodules, nested repositories, and automatic hooks are rejected. Theme imports
stay within their checkout even after it is unregistered. Refresh caches catalog
content by generation; worker tasks keep Git transfers away from the model thread.
Library controls expose semantic roles `content-library-field`,
`content-library-action`, `content-library-information`, and
`content-library-section`, with the `content-repositories` value adapter and
`content-repositories-editor` native renderer. User layouts and styles can target
these independently of library definitions.

Registry version 1 lives at `~/.tcad/content-repositories.json`. No credentials enter model files.
Version 1 folder preferences live at `~/.tcad/content-library-preferences.json`.
Pack descriptors are versioned application-owned data, separate from the desktop
renderer; their source URLs, kinds, labels, and folder names also drive the semantic
download actions.

Saved profiles/materials/hardware retain their copied definitions. Repository
finish and texture selections copy exact image bytes into project resources.
Legacy saved built-in map references can resolve the six original stable texture
IDs after the texture library is installed; a missing legacy image is reported
while the finish renders without grain. TCAD does not replace it with invented
image detail.

## Validation

On 2026-10-17, `cargo fmt --all --check`, `git diff --check`,
`cargo clippy --workspace --lib --bins --locked -- -D warnings`, and
`scripts/package-macos.sh` passed. The package step compiled the workspace,
verified the pinned helper checksum, and verified the macOS bundle signatures.
The initial integration also passed `cargo check --workspace --lib --bins --locked`.
Rust reports an existing future-compatibility notice in `proc-macro-error2` 2.0.1.

The pack-download and graceful-fallback update passed the same formatting,
whitespace, strict workspace Clippy, and signed-package checks on 2026-10-17.
Static review covered offline reuse, HTTPS/SSH connection scope, occupied folders,
queued-download cancellation, missing optional assets, and draft preservation.

No code tests, runtime probes, app launches, or manual acceptance sessions were
performed. Interactive acceptance stays with the user.
