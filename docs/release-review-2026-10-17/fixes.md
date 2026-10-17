# Release review implementation follow-up — October 17, 2026

The non-Astra code findings from the [test drive](report.md) are implemented.
Implementation commit: `25f5c42` on `codex/modeling-workflow-reliability`.
This follow-up records source changes, not new interactive acceptance. The saved
review projects remain unchanged. No automated tests or manual smoke tests were
added or run during this implementation.

| Findings | Change |
| --- | --- |
| RR-02 | Optional content packs keep their catalog order while downloads and installation status change. Focus no longer moves the selected pack to the top. |
| RR-07 | Fillet and Chamfer expose a compatible edge filter immediately. All/Edges remain selectable; incompatible filters explain the operation's requirement. The previous personal filter remains available after the tool closes. |
| RR-09, RR-10 | Sheet commands consume a compatible selected native base or terminal edge. The geometry menu includes a Sheet Metal group. Unsuitable edges explain the required selection instead of silently choosing the first base member. |
| RR-11 | Display labels resolve legacy Part1 wrappers to meaningful definitions, preserve explicit occurrence names, distinguish duplicates and include parent context in inspection and reference choices. Saved identities and pinned definitions are unchanged. |
| RR-12 | Flat-pattern DXF uses exact LINE entities for straight spans, readable cut/bend/note layers, signed bend notes and separate stable source metadata. |
| RR-13, RR-29 | Source identities and drawing controls are cached with source/context invalidation. BOM and cut-list reports share one bounded background calculation and discard stale results. Field reads avoid rebuilding complete panels. |
| RR-14 | New drawing sheets allocate rows and columns according to projected view extents and choose a fitting shared standard scale. Existing authored layouts keep their settings. |
| RR-16 | New dimensions start outside the view geometry and avoid existing text where space permits. Dragging uses the original press position, with distinct dimension grips. Existing dimensions remain editable. |
| RR-19 | Cut-list CSV headings include units, and length exports include an explicit reference-path/blank basis. They do not imply long-point saw dimensions. |
| RR-20 | Name-only component edits reuse an exact current validated evaluation. Production-preference-only edits use the same safe path. Geometry-changing edits and stale/invalid evaluations retain normal worker validation. |
| RR-21 | Pattern direction arrows use a compact, viewport-bounded display length. Model pitch stays authoritative, and themes can set the guide's maximum width. |
| RR-24 | Workspace offers Choose Theme separately from Advanced Layout and Style. Both routes remain available through semantic commands. |
| RR-25 | Publish/Save/Close actions stay above the native form's scrolling contents. Fields expose their own styleable role, labels are shorter and descriptions are bounded to the panel. |
| RR-27 | The Material tree action opens the engineering material editor. Switching from Appearance hides its native panel; shared draft scope is explained so one visible completion group handles both kinds of changes. |
| RR-31 | Each linked source groups its name, path, revisions and actions. Update previews lead with engineering changes; exact source differences remain under Advanced Source. |
| RR-32 | Applying a STEP import selects its imported part, or assembly scope for multiple parts, and requests framing. The empty Part1 prompt no longer remains the active task after acceptance. |
| Theme audit T4, T5 | Native menus and command tabs scroll when they overflow. Narrow Settings uses a compact category picker, scrollable content and wrapping confirmation actions. |
| Theme audit T7 | A live repository theme must pass full validation before replacing the saved snapshot. Invalid or missing repository content retains the saved fallback with a recovery notice. |

Source and display caches are disposable. A session source epoch changes with
edits, Apply, Cancel and undo/redo, including edits made after a restored
checkpoint. Cache reuse does not bypass mutation validation or the current
validated-evaluation requirement for Apply.

## Separate work

- **Astra scope:** native miter weld construction (RR-26), exact contact and
  interference classification (RR-30), and drawing hidden-line classification
  (RR-18).
- **Distribution decision:** the starter packs remain private (RR-03). This
  implementation does not change repository visibility. Default/fallback
  content and routes to alternate repositories remain available.
- **Acceptance:** the wider workflows marked Not run in [coverage.md](coverage.md)
  still need review. Independent DXF interoperability, hosted Git retry/conflict
  cases, physical trackpad comfort and broad geometry variants are not established
  by source changes or successful compilation.

## Validation

- `cargo fmt --all --check`: passed.
- `cargo build --workspace --locked`: passed.
- `cargo clippy --workspace --bins --lib --locked -- -D warnings`: passed.
- `git diff --check`: passed.
- `scripts/package-macos.sh`: passed; rebuilt and signature-verified
  `/Users/tyler/Software/Personal/tcad/dist/TCAD.app` from `25f5c42`.
- No automated tests or manual smoke tests added or run. No UI responsiveness
  measurement, new interactive acceptance or independent export import is claimed.

The existing dependency `proc-macro-error2` still emits its future Rust
compatibility notice; this pass introduced no new compilation or lint warnings.
